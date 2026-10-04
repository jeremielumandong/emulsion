#!/usr/bin/env python3
"""Independently verify design_finishing_workflows' PNG/PDF deliverables.

Usage: python3 scripts/verify-design-finishing.py NEW_DIRECTORY
Requires Pillow, pypdf and PyMuPDF (fitz). No source projects are modified.
The Rust example checks exact native PNG samples and editable source/history;
this verifier independently parses PDFs and checks page count, physical size,
rendered page order and bounded visual error against those verified PNGs.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import statistics
import zipfile

from PIL import Image, ImageChops, ImageStat
import fitz
from pypdf import PdfReader


EXPECTED = {"garden-vows": 2, "field-notes": 3, "market-day": 1, "studio-brief": 5}
EDGE = 720
# Vector/PDF and native glyph antialiasing differ at edges. Mean and high-error
# area bound real appearance loss while tolerating that expected difference.
MAX_MEAN_ERROR = 12.0
MAX_LARGE_ERROR_FRACTION = 0.12


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def pdf_images(resources, visited=None):
    """Resolve nested PDF Form XObjects, including full-page fallback images."""
    if visited is None:
        visited = set()
    resources = resources.get_object()
    for reference in resources.get("/XObject", {}).get_object().values() if resources.get("/XObject") else []:
        key = (getattr(reference, "idnum", None), getattr(reference, "generation", None))
        if key in visited:
            continue
        visited.add(key)
        obj = reference.get_object()
        if obj.get("/Subtype") == "/Image":
            yield int(obj["/Width"]), int(obj["/Height"])
        elif obj.get("/Subtype") == "/Form" and "/Resources" in obj:
            yield from pdf_images(obj["/Resources"], visited)


def compare(actual: Image.Image, reference: Image.Image) -> tuple[float, float]:
    actual = actual.convert("RGB")
    reference = reference.convert("RGB").resize(actual.size, Image.Resampling.LANCZOS)
    difference = ImageChops.difference(actual, reference)
    mean = statistics.mean(ImageStat.Stat(difference).mean)
    # Compare pixels rather than arbitrarily selecting a few favorable samples.
    red, green, blue = difference.split()
    maximum = ImageChops.lighter(ImageChops.lighter(red, green), blue)
    large = sum(maximum.histogram()[65:]) / (actual.width * actual.height)
    return mean, large


def verify_export(folder: Path, export: dict, all_pages: list[dict]) -> dict:
    pdf_path = folder / export["pdf"]
    reader = PdfReader(pdf_path)
    rendered = fitz.open(pdf_path)
    expected = export["pages"]
    check(len(reader.pages) == len(rendered) == len(expected), f"{pdf_path}: wrong page count")
    positions = [page["position"] for page in expected]
    check(positions == sorted(positions), f"{pdf_path}: selection order replaced project order")
    results = []
    with zipfile.ZipFile(folder / export["png_archive"]) as archive:
        names = [f"page-{page['position']:03}-{page['id']}.png" for page in expected]
        check(archive.namelist() == names, f"{pdf_path}: wrong PNG filename/order")
        for index, (pdf_page, page) in enumerate(zip(reader.pages, expected)):
            label = f"{pdf_path.name} page {index + 1} ({page['name']})"
            check(pdf_page.get("/Rotate", 0) == 0, f"{label}: unexpected rotation")
            for box_name in ("mediabox", "trimbox", "bleedbox"):
                box = getattr(pdf_page, box_name)
                check(abs(float(box.width) - page["pdf_points"][0]) < 0.02
                      and abs(float(box.height) - page["pdf_points"][1]) < 0.02,
                      f"{label}: {box_name} changed physical dimensions")
            png = Image.open(io.BytesIO(archive.read(names[index])))
            check(png.size == (page["width"], page["height"]), f"{label}: PNG dimensions")
            dpi = png.info.get("dpi")
            check(dpi is not None and all(abs(value - page["ppi"]) < 0.05 for value in dpi),
                  f"{label}: PNG physical resolution is missing/wrong: {dpi}")
            rect = rendered[index].rect
            scale = EDGE / max(rect.width, rect.height)
            pixmap = rendered[index].get_pixmap(matrix=fitz.Matrix(scale, scale), alpha=False)
            image = Image.frombytes("RGB", (pixmap.width, pixmap.height), pixmap.samples)
            mean, large = compare(image, png)
            check(mean <= MAX_MEAN_ERROR and large <= MAX_LARGE_ERROR_FRACTION,
                  f"{label}: visual error mean={mean:.4f}, large-error fraction={large:.4%}")
            # Check output content/order independently of a manifest's label.
            distances = [(compare(image, Image.open(folder / candidate["png"]))[0], candidate["id"])
                         for candidate in all_pages]
            nearest_error, nearest_id = min(distances)
            check(nearest_id == page["id"],
                  f"{label}: looks like page {nearest_id}; expected {page['id']} (best error {nearest_error:.3f})")
            images = list(pdf_images(pdf_page["/Resources"]))
            if page["name"] in export["rasterized_pages"]:
                check((page["width"], page["height"]) in images,
                      f"{label}: fallback image does not retain native pixel size / PPI")
            image.save(folder / f"{Path(export['pdf']).stem}-page-{index + 1:02}-pdf-proof.png")
            results.append({"page_id": page["id"], "name": page["name"], "mean_rgb_error": mean,
                            "large_error_fraction": large, "nearest_native_page_id": nearest_id,
                            "physical_size_points": page["pdf_points"], "embedded_image_sizes": images})
    rendered.close()
    return {"pdf": export["pdf"], "passed": True, "pages": results}


def verify(root: Path, allow_partial: bool = False) -> dict:
    manifest = json.loads((root / "verification.json").read_text())
    briefs = manifest["briefs"]
    actual = {brief["brief"]: brief["expected_pages"] for brief in briefs}
    check(bool(actual) and all(EXPECTED.get(name) == pages for name, pages in actual.items()),
          "Unexpected brief or incorrect expected page count")
    check(allow_partial or actual == EXPECTED, "The four complete finishing briefs are not present")
    results = []
    for brief in briefs:
        folder = root / brief["brief"]
        project = folder / brief["final_project"]
        check(hashlib.sha256(project.read_bytes()).hexdigest() == brief["final_sha256"],
              f"{project}: native source changed after verification")
        for field in ("input_unchanged", "round_trip_exact", "reopened_edit_undo_passed",
                      "resize_copy_undo_redo_passed", "resize_original_source_preservation_passed"):
            check(brief[field] is True, f"{brief['brief']}: {field} did not pass")
        all_pages = brief["exports"][0]["pages"]
        check(len(all_pages) == brief["expected_pages"], "Incorrect finished page count")
        for page in brief["sources"]:
            sources = page["editable_sources"]
            check(sources["text"] and sources["fonts"], "Editable text / embedded fonts missing")
        if brief["brief"] == "field-notes":
            probe = brief["long_text_resize"]
            check(probe["exact_content_preserved"] and probe["warning_reported"],
                  "Long-text narrow-resize warning/source preservation did not pass")
        results.append({"brief": brief["brief"], "passed": True, "exports": [
            verify_export(folder, export, all_pages) for export in brief["exports"]]})
        print(f"{brief['brief']}: PDF page count/order/physical size, PNG PPI and appearance passed")
    result = {"passed": True, "briefs": results,
              "thresholds": {"mean_rgb_error": MAX_MEAN_ERROR, "large_error_fraction": MAX_LARGE_ERROR_FRACTION},
              "scope": "Independent PDF parsing/rendering; exact editable state and PNG samples checked by Rust example"}
    (root / "independent-export-verification.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--allow-partial", action="store_true",
                        help="Verify an intentionally isolated --brief run")
    args = parser.parse_args()
    verify(args.directory, args.allow_partial)
    print("All independent export assertions passed.")


if __name__ == "__main__":
    main()
