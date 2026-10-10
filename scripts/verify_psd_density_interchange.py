#!/usr/bin/env python3
"""Independent byte-boundary supplement to the thirteen-vector interchange suite.

Run on NEW_DIRECTORY/density-boundaries from psd_vector_interchange. Uses the
same Pillow/numpy/psd-tools prerequisites. Reports native f32 bits, independently
derived half-away density bytes, actual export rounding counts, exact native
history/source preservation, and mandatory negative controls. No third-party renderer
or feather parity is claimed. No files in the packet are changed.
"""
from __future__ import annotations

import argparse
import copy
import io
import json
import math
from pathlib import Path
import struct
import sys
import zipfile

import numpy as np
from PIL import Image
import psd_tools
from psd_tools import PSDImage

from verify_psd_vector_interchange import (
    check, density_expectation, digest, mask_record, raw_layers, reference,
    verify_export, verify_native,
)


# Independently specified IEEE-754 inputs, including the exact adjacent f32s
# straddling 229.5 / 255. The generator's byte fields are never the oracle.
CASES = {
    "Raster below half": (0x3f666666, None),
    "Raster above half": (0x3f666667, None),
    "Carried lower raster upper vector": (0x3f666666, 0x3f666667),
    "Carried upper raster lower vector": (0x3f666667, 0x3f666666),
    "Exact half carried": (0x3f000000, 0x3f000000),
    "Byte grid carried": (0x3f19999a, 0x3f4ccccd),
}


def f32_bits(value):
    return struct.unpack(">I", struct.pack(">f", value))[0]


def requirements(manifest):
    check(manifest["schema"] == 1 and manifest["kind"] == "independent-density-boundary-interchange",
          "Unknown density boundary manifest")
    check(manifest["canvas"] == [128, 80], "Density fixture canvas")
    check(sorted(item["version"] for item in manifest["exports"]) == [1, 2], "PSD and PSB required")
    entries = {item["name"]: item for item in manifest["layers"]}
    check(len(manifest["layers"]) == len(entries) == 7 and set(entries) == set(CASES) | {"Density background"},
          "All six separate density cases and background required")
    check("raster_mask" not in entries["Density background"] and "vector_mask" not in entries["Density background"],
          "Background must remain unmasked")
    densities = {}
    for name, (raster_bits, vector_bits) in CASES.items():
        entry = entries[name]
        check(entry["kind"] == "raster" and entry["parent"] is None, f"{name}: independent raster layer required")
        check(entry["raster_mask"]["density_f32_bits"] == raster_bits, f"{name}: pinned raster f32 input")
        check(("vector_mask" in entry) == (vector_bits is not None), f"{name}: carried/vector-only confusion")
        fields = {"raster_mask": density_expectation(entry["raster_mask"], name + " raster")}
        if vector_bits is not None:
            check(entry["vector_mask"]["density_f32_bits"] == vector_bits, f"{name}: pinned vector f32 input")
            fields["vector_mask"] = density_expectation(entry["vector_mask"], name + " vector")
        for field in fields:
            check(entry[field]["feather"] is None and entry[field]["enabled"] is True,
                  f"{name}: density boundary must not be obscured by feather/disable")
        densities[name] = fields
    check(densities["Raster below half"]["raster_mask"]["expected_byte"] == 229
          and densities["Raster above half"]["raster_mask"]["expected_byte"] == 230,
          "Adjacent f32s must independently straddle the byte boundary")
    check(densities["Exact half carried"]["vector_mask"]["expected_byte"] == 128,
          "Exact 127.5 tie must round away from zero")
    expected_count = sum(field["rounded"] for fields in densities.values() for field in fields.values())
    check(expected_count == 8 and not any(field["rounded"] for field in densities["Byte grid carried"].values()),
          "Eight rounded fields and idempotent byte-grid controls required")
    return densities, expected_count


def history_plane(members, plane, raster):
    """Independent native tile decoder, retaining all RGBA16 source words."""
    width, height = plane["width"], plane["height"]
    check(0 < width <= 142 and 0 < height <= 95, "Unexpected native plane size")
    channels = 4 if raster else 1
    pixels = np.empty((height, width, channels), dtype=np.uint16 if raster else np.uint8)
    pixels[:] = plane["fill"]
    seen = set()
    for tx, ty, blob in plane["tiles"]:
        check((tx, ty) not in seen and tx >= 0 and ty >= 0, "Duplicate/negative native tile")
        seen.add((tx, ty))
        data = members[f"history/tiles/{'r' if raster else 'm'}{blob}"]
        check(len(data) == 256 * 256 * channels * (2 if raster else 1), "Native tile length")
        tile = np.frombuffer(data, dtype="<u2" if raster else "u1").reshape(256, 256, channels)
        left, top = tx * 256, ty * 256
        check(left < width and top < height, "Out-of-bounds native tile")
        right, bottom = min(left + 256, width), min(top + 256, height)
        pixels[top:bottom, left:right] = tile[:bottom - top, :right - left]
    return pixels.astype(">u2" if raster else "u1").tobytes()


def native_history(folder, manifest):
    report = verify_native(folder, manifest)
    with zipfile.ZipFile(folder / manifest["native_archives"][0]["file"]) as archive:
        members = {name: archive.read(name) for name in archive.namelist()}
    live = json.loads(members["emulsion.json"])
    graph = json.loads(members["history/graph.json"])
    check(graph["version"] >= 14, "Explicit PhotoshopSrgbV1 history requires native version 14")
    check(len(graph["commits"]) == manifest["native_history_commits"] == 2, "Two native history states required")
    check(not graph.get("working") and graph["live"], "Native history must bind the live head")
    fingerprint = 0xcbf29ce484222325
    for byte in members["emulsion.json"]:
        fingerprint = ((fingerprint ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    check(graph["live"] == f"{fingerprint:016x}", "Native live/history association")
    commits = sorted(graph["commits"], key=lambda item: item["id"])
    check(commits[0]["parents"] == [] and commits[1]["parents"] == [commits[0]["id"]], "Native history edge")
    check(graph["branches"][graph["head"]]["tip"] == commits[1]["id"], "Native authored head")
    expected = {item["name"]: item for item in manifest["layers"]}
    path = b"EMP1" + struct.pack("<IBI", 1, 1, 4)
    path += b"".join(struct.pack("<Bdd", 0, x, y) for x, y in [(8, 6), (40, 6), (40, 26), (8, 26)])
    source_digests = {}
    for state, authored, is_live in [(live, True, True), (commits[0]["doc"], False, False),
                                      (commits[1]["doc"], True, False)]:
        check(state["blend_space"] == manifest["blend_space"] == "photoshop-srgb-v1",
              "Live and both historical states must retain the explicit PhotoshopSrgbV1 contract")
        nodes = {node["name"]: node for node in state["nodes"]}
        check(len(nodes) == len(state["nodes"]) == len(expected) and set(nodes) == set(expected), "Native state layers")
        for name, node in nodes.items():
            entry = expected[name]
            kind = node["kind"]
            check(kind["type"] == "raster", f"{name}: native source type")
            source = reference(folder, entry["source"])
            words = (folder / entry["native_source"]["file"]).read_bytes()
            check(digest(words) == entry["native_source"]["sha256"]
                  and len(words) == source.shape[0] * source.shape[1] * 8, f"{name}: native RGBA16 reference")
            source_digests[name] = digest(words)
            if is_live:
                image = Image.open(io.BytesIO(members[kind["src"]]))
                check(image.mode == "RGBA" and image.size == (source.shape[1], source.shape[0])
                      and image.tobytes() == source.tobytes(), f"{name}: native source PNG")
            else:
                plane = graph["rasters"][kind["raster"]]
                check([plane["width"], plane["height"]] == entry["source"]["size"]
                      and history_plane(members, plane, True) == words, f"{name}: exact historical source words")
            if name not in CASES:
                check(node["mask"] is None and node.get("vector_mask") is None, "Native background mask")
                continue
            raster_bits, vector_bits = CASES[name]
            density = node.get("mask_properties", {}).get("density", 1.0)
            check(f32_bits(density) == (raster_bits if authored else 0x3f800000), f"{name}: exact native raster f32")
            mask = reference(folder, entry["raster_mask"])
            if is_live:
                stored = Image.open(io.BytesIO(members[node["mask"]]))
                check(stored.mode == "L" and stored.size == (mask.shape[1], mask.shape[0])
                      and stored.tobytes() == mask.tobytes(), f"{name}: exact native mask pixels")
            else:
                plane = graph["masks"][node["mask"]]
                check([plane["width"], plane["height"]] == entry["raster_mask"]["size"]
                      and history_plane(members, plane, False) == mask.tobytes(), f"{name}: exact historical mask pixels")
            vector = node.get("vector_mask")
            check((vector is not None) == (vector_bits is not None), f"{name}: native vector presence")
            if vector is not None:
                check(f32_bits(vector["properties"]["density"]) == (vector_bits if authored else 0x3f800000),
                      f"{name}: exact native vector f32")
                check(members[vector["path"]] == path and vector["transform"] == [1, 0, 0, 1, 0, 0],
                      f"{name}: exact historical editable path")
    report.update({"history_commits": 2, "exact_live_and_history_f32_bits_checked": True,
                   "exact_history_source_rgba16_sha256": source_digests})
    return report


def must_reject(action, expected_message):
    try:
        action()
    except AssertionError as error:
        check(expected_message in str(error), f"Negative control failed for unrelated reason: {error}")
        return str(error)
    raise AssertionError("Negative control was accepted: " + expected_message)


def negative_controls(folder, manifest, export):
    """Mutate copies of actual raw headers, never packet files or references."""
    psd = PSDImage.open(folder / export["file"])
    raw = raw_layers(folder / export["file"], psd.version)
    layers = {layer.name: layer for layer in psd.descendants()}
    entries = {entry["name"]: entry for entry in manifest["layers"]}
    results = []
    for field, name, flag, parameter in [
        ("raster_mask", "Raster below half", 1, "user_mask_density"),
        ("vector_mask", "Carried upper raster lower vector", 4, "vector_mask_density"),
    ]:
        entry = entries[name]
        native = struct.unpack(">f", struct.pack(">I", entry[field]["density_f32_bits"]))[0]
        f32_product = struct.unpack(">f", struct.pack(">f", native * 255))[0]
        wrong_byte = math.floor(f32_product + 0.5)
        check(f32_product == 229.5 and wrong_byte == 230, "Wrong-f32-multiply control must expose invented tie")
        wrong_manifest = copy.deepcopy(entry)
        wrong_manifest[field]["density_byte"] = wrong_byte
        metadata_error = must_reject(lambda: density_expectation(wrong_manifest[field], name),
                                     "manifest density byte disagrees")
        record = dict(raw[name])
        header = bytearray(record["mask"])
        check(header[17] & 16 and header[18] & flag, "Negative control density parameter is absent")
        offset = 19 + sum(size for bit, size in [(1, 1), (2, 8), (4, 1), (8, 8)]
                          if bit < flag and header[18] & bit)
        check(header[offset] == 229, "Negative control requires actual stored byte 229")
        header[offset] = wrong_byte
        record["mask"] = bytes(header)
        byte_error = must_reject(lambda: mask_record(folder, psd, layers[name], record, entry, name),
                                "raw " + parameter + ": 230")
        results.append({"mask": field, "layer": name, "wrong_f32_product": f32_product,
                        "wrong_byte": wrong_byte, "manifest_mutation_rejected": metadata_error,
                        "raw_header_byte_mutation_rejected": byte_error})
    return results


def verify(folder):
    manifest = json.loads((folder / "manifest.json").read_text())
    densities, count = requirements(manifest)
    native = native_history(folder, manifest)
    original = reference(folder, manifest["native_unrounded_composite"])
    represented = reference(folder, manifest["composite"])
    check(original.shape == represented.shape and np.any(original != represented), "Preview must expose density rounding")
    results = []
    for export in manifest["exports"]:
        check(digest((folder / export["file"]).read_bytes()) == export["sha256"], "Export digest")
        report = export["write_report"]
        check(report["appearance_fallback"] is None and report["baked_raster_masks"] is False,
              "Actual writer report must retain independent editable layers")
        check(type(report["rounded_mask_densities"]) is int and report["rounded_mask_densities"] == count,
              "Actual WriteReport count disagrees with independently derived native f32 changes")
        result = verify_export(folder, manifest, export)
        result["actual_write_report"] = report
        result["independently_expected_rounded_fields"] = count
        result["negative_controls"] = negative_controls(folder, manifest, export)
        results.append(result)
    return {"status": "passed", "reader": {"name": "psd-tools", "version": psd_tools.__version__, "module": psd_tools.__file__},
            "scope": "Six separate density cases; independent native-f32 byte oracle, raw raster/vector fields, exact native history and sources; saved preview and fresh geometry checked separately; no third-party renderer parity",
            "density_expectations": densities, "native_immutability": native, "exports": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--report", type=Path, help="Write a new report; existing reports are never overwritten")
    args = parser.parse_args()
    output = json.dumps(verify(args.directory), indent=2) + "\n"
    if args.report:
        with args.report.open("x") as report:
            report.write(output)
    print(output, end="")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, ValueError, KeyError, struct.error, zipfile.BadZipFile) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
