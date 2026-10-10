#!/usr/bin/env python3
"""Verify psd_mask_interchange exports with an independent PSD/PSB reader.

Generate: cargo run -p emulsion-io --example psd_mask_interchange -- NEW_DIRECTORY
Verify:  python3 scripts/verify-psd-raster-masks.py NEW_DIRECTORY
Requires Pillow and psd-tools 1.23.0 or later. A source installation can be used:
  PYTHONPATH=../dependency-source/psd-tools-1.23.0 python3 scripts/verify-psd-raster-masks.py NEW_DIRECTORY

Checks raw user-mask (-2) pixels, independent bounds, standard parameters/flags,
unchanged layer pixels and the saved merged preview. Does not composite layers,
compare different feather kernels, or claim third-party rendering verification.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

from PIL import Image
import psd_tools
from psd_tools import PSDImage


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def reference(folder: Path, expected: dict) -> bytes:
    with Image.open(folder / expected["png"]) as image:
        check(image.mode == expected["mode"], f"{expected['png']}: reference mode")
        check(list(image.size) == expected["size"], f"{expected['png']}: reference dimensions")
        pixels = image.tobytes()
    check(digest(pixels) == expected["sha256"], f"{expected['png']}: reference hash")
    return pixels


def verify_mask(folder: Path, psd, layer, expected: dict, label: str) -> dict:
    mask = layer.mask
    check(mask is not None, f"{label}: editable raster mask is missing")
    data = mask.data
    bounds = [data.left, data.top, data.right, data.bottom]
    check(bounds == expected["bounds"], f"{label}: mask bounds {bounds}")
    check(data.background_color == expected["fill"], f"{label}: outside fill")
    check(mask.disabled == (not expected["enabled"]), f"{label}: enabled state")
    # PSD bit 0 historically says 'relative to layer'; interoperable readers
    # use it as the unlinked state, with bounds still in document coordinates.
    check(mask.flags.pos_relative_to_layer == (not expected["linked"]), f"{label}: link state")
    check(mask.flags.tobytes() == bytes([expected["flags_byte"]]), f"{label}: mask flag byte")
    parameters = mask.parameters
    density = parameters.user_mask_density if parameters else None
    feather = parameters.user_mask_feather if parameters else None
    check(density == expected["density_byte"], f"{label}: density {density}")
    check(feather == expected["feather"], f"{label}: feather {feather}")
    if parameters:
        check(parameters.vector_mask_density is None and parameters.vector_mask_feather is None,
              f"{label}: unexpected vector-mask parameters")
    ids = [int(info.id) for info in layer._record.channel_info]
    check(ids.count(-2) == 1 and -3 not in ids, f"{label}: expected one -2 and no -3 channel: {ids}")
    pixels = reference(folder, expected)
    width, height = expected["size"]
    stored = layer._channels[ids.index(-2)].get_data(width, height, psd.depth, psd.version)
    check(stored == pixels, f"{label}: raw -2 channel pixels changed")
    # real=True may choose the combined -3 channel. Inspect the raw user grid.
    image = mask.topil(real=False, layer_sized=False)
    check(image is not None and image.mode == "L", f"{label}: decoded mask mode")
    check(list(image.size) == expected["size"], f"{label}: mask cropped to layer")
    check(image.tobytes() == pixels, f"{label}: decoded mask pixels changed")
    return {"bounds": bounds, "sha256": digest(stored), "channel_id": -2,
            "flags_byte": expected["flags_byte"], "density_byte": density, "feather": feather}


def verify_export(folder: Path, manifest: dict, export: dict) -> dict:
    psd = PSDImage.open(folder / export["file"])
    label = export["file"]
    check(psd.version == export["version"], f"{label}: wrong PSD/PSB version")
    check(list(psd.size) == manifest["canvas"] and psd.depth == 8, f"{label}: canvas/depth")
    actual = list(psd.descendants())
    layers = {layer.name: layer for layer in actual}
    expected_names = [entry["name"] for entry in manifest["layers"]]
    check(len(layers) == len(actual) == len(expected_names), f"{label}: duplicate or missing layer")
    check(set(layers) == set(expected_names), f"{label}: layer names or hierarchy changed")
    for siblings in manifest["sibling_order"]:
        container = psd if siblings["parent"] is None else layers[siblings["parent"]]
        # psd-tools GroupMixin.append documents the end as the top; iteration
        # yields that same stored bottom-to-top list (also used by its compositor).
        check([layer.name for layer in container] == siblings["bottom_to_top"],
              f"{label}: sibling stacking order under {siblings['parent']!r}")
    results = []
    for entry in manifest["layers"]:
        layer = layers[entry["name"]]
        context = f"{label}: {layer.name}"
        check(layer.is_group() == (entry["kind"] == "group"), f"{context}: kind")
        parent = None if layer.parent is psd else layer.parent.name
        check(parent == entry["parent"], f"{context}: parent {parent}")
        result = {"name": layer.name, "kind": entry["kind"]}
        if "source" in entry:
            source = entry["source"]
            record = layer._record
            check([record.left, record.top, record.right, record.bottom] == source["bounds"],
                  f"{context}: source bounds")
            pixels = reference(folder, source)
            image = layer.topil(apply_icc=False)
            check(image is not None and list(image.size) == source["size"], f"{context}: source dimensions")
            check(image.convert("RGBA").tobytes() == pixels, f"{context}: source pixels/alpha changed")
            result["source_sha256"] = digest(pixels)
        if "mask" in entry:
            result["mask"] = verify_mask(folder, psd, layer, entry["mask"], context)
        else:
            check(layer.mask is None, f"{context}: unexpected mask")
        results.append(result)
    expected = reference(folder, manifest["composite"])
    check(all(alpha == 255 for alpha in expected[3::4]), "Reference composite must be opaque")
    # The fixture's opaque background avoids reader-specific white-matte alpha
    # correction. This is the saved preview, never psd.composite() rendering.
    preview = psd.topil(apply_icc=False)
    check(preview is not None and list(preview.size) == manifest["canvas"], f"{label}: merged preview")
    check(preview.convert("RGBA").tobytes() == expected, f"{label}: stored merged pixels changed")
    return {"file": label, "version": psd.version, "layers": results,
            "sibling_order": manifest["sibling_order"],
            "stored_composite_sha256": digest(expected)}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--report", type=Path, help="Optionally save the JSON verification report")
    args = parser.parse_args()
    manifest = json.loads((args.directory / "manifest.json").read_text())
    check(manifest["schema"] == 1, "Unknown manifest schema")
    check(sorted(export["version"] for export in manifest["exports"]) == [1, 2],
          "Fixture must include both PSD and PSB")
    masks = [entry["mask"] for entry in manifest["layers"] if "mask" in entry]
    check(len(masks) == 6, "Fixture must exercise six editable masks")
    check(sum(entry["kind"] == "group" for entry in manifest["layers"] if "mask" in entry) == 2,
          "Fixture must exercise two masked groups")
    for field in ("enabled", "linked"):
        check({mask[field] for mask in masks} == {False, True}, f"Missing {field} coverage")
    check({mask["fill"] for mask in masks} == {0, 255}, "Missing outside-fill coverage")
    check(any(mask["density_byte"] == 153 and mask["feather"] == 2.25 for mask in masks),
          "Missing density/feather coverage")
    width, height = manifest["canvas"]
    check(any(mask["bounds"][0] < 0 and mask["bounds"][1] < 0
              and mask["bounds"][2] > width and mask["bounds"][3] > height for mask in masks),
          "Missing off-canvas grid coverage")
    report = {
        "status": "passed",
        "reader": {"name": "psd-tools", "version": psd_tools.__version__, "module": psd_tools.__file__},
        "scope": "Stored editable -2 raster masks, layer pixels and opaque saved preview; no third-party rendering or feather-kernel parity claim",
        "exports": [verify_export(args.directory, manifest, export) for export in manifest["exports"]],
    }
    text = json.dumps(report, indent=2) + "\n"
    if args.report:
        args.report.write_text(text)
    print(text, end="")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
