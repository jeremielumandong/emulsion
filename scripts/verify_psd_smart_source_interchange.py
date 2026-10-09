#!/usr/bin/env python3
"""Verify source-only Smart PSD/PSB artifacts with psd-tools and native ZIP data.

Generate with the psd_smart_source_interchange Rust example. Requires psd-tools
1.23.0, Pillow and numpy; a PYTHONPATH source installation is sufficient.

The original PNG proof is the actual liFD embedded payload selected by SoLd
Idnt, independently checked against the pinned MIT fixture and an exact SHA-256.
The separate placed instance ID and source-sized transforms must agree. Layer
channels are checked only for unbaked preview pixels, never as original-source
evidence; no merged preview is used. Native RGBA16 history tiles independently
establish the canonical source digest. This is not third-party application or
smart-filter/filter-mask acceptance evidence.

Record definitions: the published PSD/PSB file format specification (Linked Layer, Placed Layer
Data, Layer Mask Data and Vector Mask Setting):
https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
Idnt association and embedded data interpretation follow the independent reader:
https://github.com/psd-tools/psd-tools/blob/d68bf46c7140a1f8c74be9c10b4e21103e820761/src/psd_tools/api/smart_object.py
https://github.com/psd-tools/psd-tools/blob/d68bf46c7140a1f8c74be9c10b4e21103e820761/src/psd_tools/psd/linked_layer.py
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import sys
import uuid
import zipfile

import numpy as np
from PIL import Image
import psd_tools
from psd_tools import PSDImage
from psd_tools.constants import LinkedLayerType, Tag


FIXTURE_SHA256 = "a34abf773a64f21b59d5a854f7d394e7ac41e402d0ce546ddd86d19c764a63a2"
PNG_SHA256 = "582ae5daa72a495b9d8fcee8593d5253f62ea268c357d0816b3bd00b5065267c"
POINTS = [[2, 3], [29, 3], [29, 28], [2, 28]]
DOMAIN = b"Emulsion OriginalImage native-source v1\0"


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verified_file(folder, entry):
    path = folder / entry["file"]
    check(digest(path.read_bytes()) == entry["sha256"], f"{path.name}: file SHA-256")
    return path


def scalar(value):
    return value.value


def identifier(value):
    text = scalar(value).rstrip("\0")
    check(str(uuid.UUID(text)) == text, "Invalid or noncanonical source/instance UUID")
    return text


def smart_records(psd, layer, origin, original, exported=False):
    """Read actual descriptor/linked records, not serialized library output."""
    label = layer.name
    keys = [key for key in (Tag.SMART_OBJECT_LAYER_DATA1, Tag.SMART_OBJECT_LAYER_DATA2)
            if key in layer.tagged_blocks]
    check(len(keys) == 1, f"{label}: one modern placed descriptor required")
    modern = layer.tagged_blocks.get_data(keys[0])
    check(modern.kind == b"soLD" and modern.version in (4, 5), f"{label}: placed record framing")
    descriptor = modern.data
    source_id = identifier(descriptor[b"Idnt"])
    instance_id = identifier(descriptor[b"placed"])
    check(source_id != instance_id, f"{label}: source and placed-instance IDs were conflated")
    if exported:
        check(all(uuid.UUID(value).version == 4 and uuid.UUID(value).variant == uuid.RFC_4122
                  for value in (source_id, instance_id)), f"{label}: generated RFC 4122 version-4 UUIDs")
    size = descriptor[b"Sz  "]
    check([scalar(size[key]) for key in (b"Wdth", b"Hght")] == [32, 32],
          f"{label}: source dimensions came from trimmed preview")
    x, y = origin
    transform = [x, y, x + 32, y, x + 32, y + 32, x, y + 32]
    check([scalar(v) for v in descriptor[b"Trnf"]] == transform, f"{label}: placed-source transform")
    if b"nonAffineTransform" in descriptor:
        check([scalar(v) for v in descriptor[b"nonAffineTransform"]] == transform,
              f"{label}: non-affine transform disagrees")
    linked = []
    for key in (Tag.LINKED_LAYER1, Tag.LINKED_LAYER2, Tag.LINKED_LAYER3, Tag.LINKED_LAYER_EXTERNAL):
        if key in psd.tagged_blocks:
            linked.extend(psd.tagged_blocks.get_data(key))
    check(len(linked) == 1, f"{label}: expected one embedded source record")
    record = linked[0]
    check(record.uuid == source_id and record.uuid != instance_id, f"{label}: Idnt/liFD association")
    check(record.kind == LinkedLayerType.DATA and 1 <= record.version <= 7,
          f"{label}: expected bounded embedded liFD record")
    check(record.filetype.strip(b" \0").lower() == b"png", f"{label}: PNG file type")
    check(record.data == original and digest(record.data) == PNG_SHA256, f"{label}: exact embedded PNG")
    smart = layer.smart_object
    check(smart.kind == "data" and smart.unique_id == source_id and smart.data == record.data,
          f"{label}: independent Smart Object API association")
    legacy_keys = [key for key in (Tag.PLACED_LAYER1, Tag.PLACED_LAYER2) if key in layer.tagged_blocks]
    for key in legacy_keys:
        legacy = layer.tagged_blocks.get_data(key)
        # The legacy PlLd UUID is the source ID, not the modern instance UUID.
        legacy_id = legacy.uuid.decode("macroman") if isinstance(legacy.uuid, bytes) else legacy.uuid
        check(legacy.kind == b"plcL" and legacy.version == 3 and legacy_id == source_id,
              f"{label}: legacy source association")
        check(list(legacy.transform) == transform, f"{label}: legacy source transform")
    return {"source_id": source_id, "placed_instance_id": instance_id,
            "encoded_sha256": digest(record.data), "embedded_bytes": len(record.data),
            "source_size": [32, 32], "transform": transform, "preview_bounds": list(layer.bbox)}


def masks(psd, layer, case, mask_pixels):
    target = case["raster_mask"]
    mask = layer.mask
    check(mask is not None and not mask.has_real(), f"{layer.name}: independent ordinary mask missing")
    check(list(mask.bbox) == target["bounds"] and mask.background_color == target["fill"],
          f"{layer.name}: source-relative ordinary mask extent/fill")
    flags = mask.flags
    check(flags.mask_disabled == (not target["enabled"])
          and flags.pos_relative_to_layer == (not target["linked"]), f"{layer.name}: raster flags")
    check(not flags.invert_mask and not flags.user_mask_from_render and not flags.parameters_applied,
          f"{layer.name}: unexpected baked/parameterized raster mask")
    ids = [int(info.id) for info in layer._record.channel_info]
    check(ids.count(-2) == 1 and -3 not in ids, f"{layer.name}: independent -2, no merged -3 mask channel")
    data = layer._channels[ids.index(-2)].get_data(36, 38, psd.depth, psd.version)
    check(data == mask_pixels and digest(data) == target["pixel_sha256"], f"{layer.name}: raster samples changed")
    vector = layer.vector_mask
    target = case["vector_mask"]
    check(vector is not None, f"{layer.name}: editable vector mask missing")
    check(vector.disabled == (not target["enabled"]) and vector.not_linked == (not target["linked"])
          and vector.inverted == target["inverted"], f"{layer.name}: vector flags")
    check(vector.initial_fill_rule == 0 and len(vector.paths) == 1, f"{layer.name}: vector fill/path count")
    path = vector.paths[0]
    check(path.is_closed() and path.operation == 1 and len(path) == 4, f"{layer.name}: simple combined rectangle")
    for knot, expected in zip(path, target["document_points"]):
        for field in ("preceding", "anchor", "leaving"):
            y, x = getattr(knot, field)
            check(abs(x * 64 - expected[0]) <= 64 / (2 * (1 << 24)) + 1e-10
                  and abs(y * 48 - expected[1]) <= 48 / (2 * (1 << 24)) + 1e-10,
                  f"{layer.name}: {field} uses preview origin instead of source origin")
    return {"raster_channel": -2, "raster_pixel_sha256": digest(data),
            "raster_bounds": list(mask.bbox), "vector_document_points": target["document_points"],
            "enabled": target["enabled"], "linked": target["linked"], "inverted": target["inverted"]}


def history_plane(members, plane, raster):
    """Decode native tile words ourselves; no Emulsion code or preview decode."""
    width, height = plane["width"], plane["height"]
    check(0 < width <= 64 and 0 < height <= 64, "Unexpected fixture native plane size")
    channels = 4 if raster else 1
    pixels = np.empty((height, width, channels), dtype=np.uint16 if raster else np.uint8)
    pixels[:] = plane["fill"]
    seen = set()
    for tx, ty, blob in plane["tiles"]:
        check((tx, ty) not in seen and tx >= 0 and ty >= 0, "Duplicate/negative native tile")
        seen.add((tx, ty))
        raw = members[f"history/tiles/{'r' if raster else 'm'}{blob}"]
        check(len(raw) == 256 * 256 * channels * (2 if raster else 1), "Native tile byte length")
        tile = np.frombuffer(raw, dtype="<u2" if raster else "u1").reshape(256, 256, channels)
        left, top = tx * 256, ty * 256
        check(left < width and top < height, "Out-of-bounds native tile")
        right, bottom = min(left + 256, width), min(top + 256, height)
        pixels[top:bottom, left:right] = tile[:bottom - top, :right - left]
    return pixels.astype(">u2" if raster else "u1").tobytes()


def native_node(members, node, source, case, live):
    kind = node["kind"]
    check(kind["type"] == "smart" and not kind.get("editable") and not kind.get("source_document"),
          "Native source lost raster-backed Smart identity")
    expected = {"encoded_sha256": PNG_SHA256, "source_sha256": source["native_source_sha256"],
                "width": 32, "height": 32}
    check(kind["original_image"] == expected, "Native original digest binding")
    check(not kind["filters"] and not kind.get("filter_styles") and not kind.get("filter_mask"),
          "Unexpected native filter/filter-mask state")
    placement = kind["placement"]
    check(placement["scale_x"] == placement["scale_y"] == 1 and placement["rotation"] == 0
          and not placement["flip_x"] and not placement["flip_y"], "Native source transform changed")
    if live:
        check([placement["x"], placement["y"]] == case["placement"], "Native source origin changed")
        check(kind["src"] == f"original-images/{PNG_SHA256}.png", "Live source must use original resource directly")
    if node.get("vector_mask") is None:
        check(not live and node["mask"] is None, "Current native masks missing")
        return
    raster, vector = case["raster_mask"], case["vector_mask"]
    check(node["mask_enabled"] == raster["enabled"] and node["mask_linked"] == raster["linked"]
          and node["mask_transform"] == raster["transform"], "Native raster mask state changed")
    stored = node["vector_mask"]
    check(all(stored[key] == vector[key] for key in ("enabled", "linked", "inverted")), "Native vector state changed")
    check(stored["transform"] == [1, 0, 0, 1, 0, 0] and stored["empty_coverage"] == "hide_all",
          "Native vector geometry convention changed")
    # Native EMP1 format: one closed subpath, four corner anchors, no handles.
    path = b"EMP1" + struct.pack("<IBI", 1, 1, 4)
    path += b"".join(struct.pack("<Bdd", 0, *point) for point in POINTS)
    check(members[stored["path"]] == path, "Native editable path resource changed")


def verify_native(folder, source, case, original, native_bytes, mask_pixels):
    archives = []
    for field in ("native_before", "native_after"):
        with zipfile.ZipFile(verified_file(folder, case[field])) as archive:
            names = archive.namelist()
            check(len(names) == len(set(names)), "Duplicate native ZIP member")
            archives.append({name: archive.read(name) for name in names})
    before, after = archives
    check(before == after, f"{case['name']}: before/after native document, resources or history changed")
    resources = [key for key in before if key.startswith("original-images/")]
    check(resources == [f"original-images/{PNG_SHA256}.png"] and before[resources[0]] == original,
          "Native shared original resource changed or was duplicated")
    manifest = json.loads(before["emulsion.json"])
    graph = json.loads(before["history/graph.json"])
    check(manifest["version"] == graph["version"] == 13, "Originals require native/history version 13")
    check(len(manifest["nodes"]) == 1 and len(graph["commits"]) == case["native_history_commits"] == 2,
          "Native live/history cardinality")
    check(not graph.get("working") and graph["live"], "Fixture history must bind live head")
    fingerprint = 0xcbf29ce484222325
    for byte in before["emulsion.json"]:
        fingerprint = ((fingerprint ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    check(graph["live"] == f"{fingerprint:016x}", "History/live manifest association")
    native_node(before, manifest["nodes"][0], source, case, True)
    live_mask = Image.open(io.BytesIO(before[manifest["nodes"][0]["mask"]]))
    check(live_mask.mode == "L" and live_mask.size == (36, 38) and live_mask.tobytes() == mask_pixels,
          "Native ordinary mask PNG changed")
    for commit in graph["commits"]:
        check(len(commit["doc"]["nodes"]) == 1, "Native historical layer cardinality")
        node = commit["doc"]["nodes"][0]
        native_node(before, node, source, case, False)
        kind = node["kind"]
        for field in ("source", "cache"):
            plane = graph["rasters"][kind[field]]
            words = history_plane(before, plane, True)
            check([plane["width"], plane["height"]] == [32, 32] and words == native_bytes,
                  f"Native history {field} RGBA16 changed")
            check(digest(DOMAIN + struct.pack(">II", 32, 32) + words) == source["native_source_sha256"],
                  "Native history canonical source digest")
        check(kind["offset"] == [0, 0], "Native history cache offset changed")
        if node["mask"] is not None:
            check(history_plane(before, graph["masks"][node["mask"]], False) == mask_pixels,
                  "Native history ordinary mask pixels changed")
    return {"before_after_members_identical": True, "member_count": len(before),
            "history_commits": len(graph["commits"]), "shared_original_resources": len(resources),
            "native_source_sha256": source["native_source_sha256"]}


def verify(folder):
    manifest = json.loads((folder / "manifest.json").read_text())
    check(manifest["schema"] == 1 and manifest["kind"] == "source-only-smart-interchange", "Unknown manifest")
    check(manifest["canvas"] == [64, 48] and manifest["input_document_source_history_unchanged"] is True,
          "Missing canvas/immutability assertion")
    source = manifest["source"]
    original = (folder / source["file"]).read_bytes()
    check(digest(original) == source["encoded_sha256"] == PNG_SHA256, "Pinned original PNG SHA-256")
    image = Image.open(io.BytesIO(original))
    check(image.mode == "RGBA" and list(image.size) == source["size"] == [32, 32], "Original PNG layout")
    pixels = np.asarray(image)
    hidden = int(np.count_nonzero((pixels[:, :, 3] == 0) & np.any(pixels[:, :, :3] != 0, axis=2)))
    partial = int(np.count_nonzero((pixels[:, :, 3] > 0) & (pixels[:, :, 3] < 255)))
    check(hidden == source["hidden_rgb_pixels"] == 95 and partial == source["partial_alpha_pixels"] == 99,
          "Original hidden-RGB/partial-alpha coverage changed")
    reencoded = Image.open(folder / source["native_reencoded_png"]).convert("RGBA")
    check(reencoded.size == image.size and reencoded.tobytes() != image.tobytes(),
          "Fixture no longer distinguishes original PNG from native re-encoding")
    native_bytes = (folder / source["native_file"]).read_bytes()
    check(len(native_bytes) == 32 * 32 * 8 and
          digest(DOMAIN + struct.pack(">II", 32, 32) + native_bytes) == source["native_source_sha256"],
          "Canonical native source reference digest")
    input_path = verified_file(folder, manifest["input"])
    check(digest(input_path.read_bytes()) == FIXTURE_SHA256, "Pinned MIT source fixture SHA-256")
    input_psd = PSDImage.open(input_path)
    check(len(input_psd) == 1 and input_psd[0].bbox == (5, 5, 27, 28), "Pinned trimmed-preview coverage")
    input_records = smart_records(input_psd, input_psd[0], [0, 0], original)
    check(input_psd[0].size != (32, 32), "Input must distinguish preview bounds from source dimensions")
    cases = manifest["cases"]
    check(len(cases) == 3, "Expected three focused mask-state cases")
    for field in ("enabled", "linked", "inverted"):
        check({case["vector_mask"][field] for case in cases} == {False, True}, f"Missing vector {field} coverage")
    for field in ("enabled", "linked"):
        check({case["raster_mask"][field] for case in cases} == {False, True}, f"Missing raster {field} coverage")
        check(any(case["raster_mask"][field] != case["vector_mask"][field] for case in cases),
              f"Raster/vector {field} must differ to expose conflated state")
    results = []
    for case in cases:
        check(case["placement"] == [11, 7] and case["source_bounds"] == [11, 7, 43, 39], "Expected translated source")
        check(case["vector_mask"]["source_points"] == POINTS
              and case["vector_mask"]["document_points"] == [[x + 11, y + 7] for x, y in POINTS], "Expected rectangle")
        mask_image = Image.open(folder / case["raster_mask"]["file"])
        mask_pixels = bytes((x * 7 + y * 11) % 256 for y in range(38) for x in range(36))
        check(mask_image.mode == "L" and mask_image.size == (36, 38) and mask_image.tobytes() == mask_pixels,
              "Independent asymmetric ordinary mask reference")
        native = verify_native(folder, source, case, original, native_bytes, mask_pixels)
        check(sorted(entry["version"] for entry in case["exports"]) == [1, 2], "Both PSD and PSB required")
        exports = []
        for entry in case["exports"]:
            check(entry["appearance_fallback"] is None and entry["baked_raster_masks"] is False, "Export report fell back/baked")
            psd = PSDImage.open(verified_file(folder, entry))
            check(psd.version == entry["version"] and psd.size == (64, 48) and psd.depth == 8 and len(psd) == 1,
                  f"{entry['file']}: PSD/PSB header and layer count")
            layer = psd[0]
            check(layer.kind == "smartobject" and layer.name == case["layer_name"], "Appearance fallback layer")
            records = smart_records(psd, layer, case["placement"], original, exported=True)
            check(list(layer.bbox) == case["source_bounds"], "Output preview uses full original-sized source grid")
            for tagged in (psd.tagged_blocks, layer.tagged_blocks):
                check(not any(key in tagged for key in (b"FEid", b"FXid", b"FMsk")), "Unexpected filter-related output")
            preview = layer.topil(apply_icc=False)
            check(preview is not None and preview.convert("RGBA").tobytes() == reencoded.tobytes(),
                  "Layer channels were masked/baked; this preview check is separate from the embedded PNG proof")
            mask_report = masks(psd, layer, case, mask_pixels)
            exports.append({"file": entry["file"], "version": psd.version,
                            "source": records, "masks": mask_report, "unbaked_preview_checked_separately": True})
        results.append({"case": case["name"], "native": native, "exports": exports})
    return {"status": "passed", "reader": {"name": "psd-tools", "version": psd_tools.__version__, "module": psd_tools.__file__},
            "scope": "Actual embedded PNG/UUID association, translated source vs trimmed input preview, ordinary raster/vector records, exact native/history resources; no third-party application or smart-filter support claim",
            "input": input_records, "hidden_rgb_pixels": hidden, "partial_alpha_pixels": partial, "cases": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--report", type=Path, help="Write a new report file; existing reports are never overwritten")
    args = parser.parse_args()
    result = json.dumps(verify(args.directory), indent=2) + "\n"
    if args.report:
        with args.report.open("x") as report:
            report.write(result)
    print(result, end="")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, ValueError, KeyError, struct.error, zipfile.BadZipFile) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
