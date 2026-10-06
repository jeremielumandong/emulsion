#!/usr/bin/env python3
"""Independently verify editable PSD/PSB vector-mask interchange.

Generate with the psd_vector_interchange Rust example. Requires Pillow, numpy
and psd-tools 1.23.0 or later (a PYTHONPATH source installation is sufficient).

The byte walker below does not use either PSD library. It checks exact path
records and a separately specified golden packet. psd-tools then verifies raw
source/mask pixels and freshly rasterizes path geometry. Inversion, disable,
density and raster-mask multiplication are applied explicitly; draw_vector_mask
itself does NOT implement those properties. Feather is checked as a stored
parameter, never advertised as cross-renderer feather-kernel parity. The saved
merged preview is checked separately and is never used as vector evidence.
"""
from __future__ import annotations

import argparse
import hashlib
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
from psd_tools.composite.vector import draw_vector_mask


SCALE = 1 << 24
# 8.24 spec example: this rectangle has x and y at exactly 1/8 or 3/8.
# The byte values are independently specified, not produced by ag-psd,
# psd-tools, or the manifest's coordinate encoder.
GOLDEN_RECTANGLE = (
    bytes.fromhex("00000003 00000000")
    + bytes.fromhex("0006") + bytes(24)
    + bytes.fromhex("0008 0000") + bytes(22)
    + bytes.fromhex("0000 0004 0001 0001") + bytes(18)
    + bytes.fromhex("0002") + bytes.fromhex("00200000 00200000") * 3
    + bytes.fromhex("0002") + bytes.fromhex("00200000 00600000") * 3
    + bytes.fromhex("0002") + bytes.fromhex("00600000 00600000") * 3
    + bytes.fromhex("0002") + bytes.fromhex("00600000 00200000") * 3
)


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def density_expectation(item, label):
    """Decode stored native f32 bits; never take a manifest byte as the oracle."""
    bits = item["density_f32_bits"]
    check(type(bits) is int and 0 <= bits <= 0xffffffff, f"{label}: invalid native f32 bits")
    native = struct.unpack(">f", struct.pack(">I", bits))[0]
    check(math.isfinite(native) and 0 <= native <= 1, f"{label}: invalid native density")
    # Python float exactly represents f32 and this product (at most 32 bits
    # of significance). floor(x + 1/2) is half away for nonnegative densities.
    # Do not round the product back to f32, or use Python's ties-to-even round.
    byte = math.floor(native * 255 + 0.5)
    represented_bits = struct.unpack(">I", struct.pack(">f", byte / 255))[0]
    parameter = None if byte == 255 else byte
    check(item["density_byte"] == parameter, f"{label}: manifest density byte disagrees with native f32 bits")
    return {"native_f32_bits": bits, "native_value": native, "scaled_exact": native * 255,
            "expected_byte": byte, "parameter_byte": parameter,
            "represented_f32_bits": represented_bits, "rounded": represented_bits != bits}


class Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def take(self, count):
        check(0 <= count <= len(self.data) - self.pos, "Truncated raw PSD structure")
        result = self.data[self.pos:self.pos + count]
        self.pos += count
        return result

    def number(self, fmt):
        return struct.unpack(">" + fmt, self.take(struct.calcsize(">" + fmt)))[0]

    def block(self, fmt="I"):
        return self.take(self.number(fmt))


def raw_layers(path, version):
    """Minimal independent record walker; no decoded-reader serialization."""
    r = Reader(path.read_bytes())
    check(r.take(4) == b"8BPS" and r.number("H") == version, "Raw PSD header/version")
    r.take(20)
    r.block()  # Color mode data.
    r.block()  # Image resources.
    lm = Reader(r.block("Q" if version == 2 else "I"))
    info = Reader(lm.block("Q" if version == 2 else "I"))
    count = abs(info.number("h"))
    results = {}
    large_keys = {b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn",
                  b"Alph", b"FMsk", b"lnk2", b"FEid", b"FXid", b"PxSD"}
    for _ in range(count):
        bounds = struct.unpack(">4i", info.take(16))
        channels = [(info.number("h"), info.number("Q" if version == 2 else "I"))
                    for _ in range(info.number("H"))]
        check(info.take(4) == b"8BIM", "Raw layer blend signature")
        info.take(8)
        extra = Reader(info.block())
        mask = extra.block()
        extra.block()  # Blending ranges.
        name_start = extra.pos
        name = extra.take(extra.number("B")).decode("macroman")
        extra.take((-(extra.pos - name_start)) % 4)
        tags = {}
        while len(extra.data) - extra.pos >= 12:
            signature = extra.take(4)
            check(signature in (b"8BIM", b"8B64"), f"{name}: raw additional-info signature")
            key = extra.take(4)
            value = extra.block("Q" if version == 2 and key in large_keys else "I")
            check(key not in tags, f"{name}: duplicate raw {key!r}")
            tags[key] = value
            extra.take((-len(value)) % 2)
        check(not any(extra.take(len(extra.data) - extra.pos)), f"{name}: trailing extra bytes")
        if b"luni" in tags:
            unicode_name = Reader(tags[b"luni"])
            name = unicode_name.take(unicode_name.number("I") * 2).decode("utf-16-be")
        # Section-divider names may repeat; ordinary fixture names may not.
        if name not in results:
            results[name] = {"bounds": bounds, "channels": channels, "mask": mask, "tags": tags}
        else:
            check(not ({b"vmsk", b"vsms"} & tags.keys()), f"{name}: duplicate vector layer")
    return results


def reference(folder, item):
    with Image.open(folder / item["png"]) as image:
        check(image.mode == item["mode"] and list(image.size) == item["size"],
              f"{item['png']}: reference mode/size")
        pixels = image.tobytes()
        array = np.asarray(image).copy()
    check(digest(pixels) == item["sha256"], f"{item['png']}: reference digest")
    return array


def rounded_fixed(value):
    check(math.isfinite(value) and -16 <= value < 16, "Coordinate outside supported 8.24 envelope")
    # Rust f64::round is half away from zero, unlike Python's round().
    return math.floor(value * SCALE + 0.5) if value >= 0 else math.ceil(value * SCALE - 0.5)


def expected_packet(vector, canvas):
    flags = int(vector["inverted"]) | (int(not vector["linked"]) << 1) | (int(not vector["enabled"]) << 2)
    packet = struct.pack(">IIH24xHH22x", 3, flags, 6, 8, vector["initial_fill"])
    for path in vector["paths"]:
        packet += struct.pack(">HHhH18x", 0 if path["closed"] else 3, len(path["anchors"]), 1, 1)
        for anchor in path["anchors"]:
            selector = (1 if anchor["smooth"] else 2) if path["closed"] else (4 if anchor["smooth"] else 5)
            coords = []
            for name in ("preceding", "anchor", "leaving"):
                x, y = anchor[name]
                coords.extend((rounded_fixed(y / canvas[1]), rounded_fixed(x / canvas[0])))
            packet += struct.pack(">H6i", selector, *coords)
    return packet


def check_packet(actual, expected, label):
    check(actual[:len(expected)] == expected, f"{label}: raw path packet differs")
    check(len(actual) - len(expected) in (0, 1, 2, 3) and not any(actual[len(expected):]),
          f"{label}: unexpected path records or nonzero padding")


def vector_record(layer, raw, expected, canvas, label):
    tags = raw["tags"]
    keys = set(tags) & {b"vmsk", b"vsms"}
    check(len(keys) == 1, f"{label}: exactly one editable vector block required")
    packet = tags[keys.pop()]
    check_packet(packet, expected_packet(expected, canvas), label)
    if layer.name == "Golden rectangle":
        check_packet(packet, GOLDEN_RECTANGLE, label + " independently specified golden")
    vector = layer.vector_mask
    check(vector is not None, f"{label}: independent reader sees no vector mask")
    check(vector.inverted == expected["inverted"], f"{label}: invert flag")
    check(vector.not_linked == (not expected["linked"]), f"{label}: link flag")
    check(vector.disabled == (not expected["enabled"]), f"{label}: disable flag")
    check(vector.initial_fill_rule == expected["initial_fill"], f"{label}: initial fill")
    check(len(vector.paths) == len(expected["paths"]), f"{label}: path count")
    for path, target in zip(vector.paths, expected["paths"]):
        check(path.is_closed() == target["closed"] and path.operation == 1,
              f"{label}: open/closed or explicit Combine=1")
        check(path._unknown1 == 1 and path._unknown2 == 0 and path.index == 0
              and path._unknown3 == bytes(10), f"{label}: subpath metadata")
        check(len(path) == len(target["anchors"]), f"{label}: knot count")
        for knot, anchor in zip(path, target["anchors"]):
            for field in ("preceding", "anchor", "leaving"):
                y, x = getattr(knot, field)
                px, py = anchor[field]
                check(abs(x * canvas[0] - px) <= canvas[0] / (2 * SCALE) + 1e-10
                      and abs(y * canvas[1] - py) <= canvas[1] / (2 * SCALE) + 1e-10,
                      f"{label}: independently decoded {field} coordinate")
    return {"raw_packet_sha256": digest(packet), "raw_packet_bytes": len(packet),
            "flags": struct.unpack(">I", packet[4:8])[0],
            "path_count": len(vector.paths), "golden_checked": layer.name == "Golden rectangle"}


def mask_record(folder, psd, layer, raw, entry, label):
    raster = entry.get("raster_mask")
    vector = entry.get("vector_mask", {})
    ids = [int(x.id) for x in layer._record.channel_info]
    check(ids == [x[0] for x in raw["channels"]], f"{label}: raw/reader channel IDs")
    check(-3 not in ids, f"{label}: unexpected combined -3 channel")
    if raster is None:
        check(-2 not in ids and layer.mask is None and not raw["mask"],
              f"{label}: synthetic raster carrier is forbidden")
        check(vector.get("density_byte") is None and vector.get("feather") is None,
              f"{label}: parameters without independent carrier")
        return None
    check(ids.count(-2) == 1 and layer.mask is not None, f"{label}: one independent -2 channel")
    body = Reader(raw["mask"])
    check(len(body.data) < 36, f"{label}: unsupported long mask parameter header")
    top, left, bottom, right = struct.unpack(">4i", body.take(16))
    fill, flags = body.number("B"), body.number("B")
    raster_density = density_expectation(raster, label + " raster")
    vector_density = density_expectation(vector, label + " vector") if vector else None
    params = {"user_mask_density": raster_density["parameter_byte"], "user_mask_feather": raster["feather"],
              "vector_mask_density": vector_density["parameter_byte"] if vector_density else None,
              "vector_mask_feather": vector.get("feather")}
    has_params = any(value is not None for value in params.values())
    expected_flags = int(not raster["linked"]) | (int(not raster["enabled"]) << 1) | (int(has_params) << 4)
    check(flags == expected_flags, f"{label}: raw raster flags")
    check([left, top, right, bottom] == raster["bounds"] and fill == raster["fill"],
          f"{label}: raw raster bounds/outside fill")
    parameter_flags = body.number("B") if has_params else 0
    check(parameter_flags == sum((1 << i) for i, value in enumerate(params.values()) if value is not None),
          f"{label}: exact separate parameter-presence bits")
    parsed = {}
    for i, (name, expected) in enumerate(params.items()):
        value = body.number("d" if "feather" in name else "B") if parameter_flags & (1 << i) else None
        check(value == expected, f"{label}: raw {name}: {value}")
        decoded = getattr(layer.mask.parameters, name) if layer.mask.parameters else None
        check(decoded == expected, f"{label}: independent reader {name}: {decoded}")
        parsed[name] = value
    check(not any(body.take(len(body.data) - body.pos)), f"{label}: unknown mask trailer")
    source = reference(folder, raster)
    width, height = raster["size"]
    stored = layer._channels[ids.index(-2)].get_data(width, height, psd.depth, psd.version)
    check(stored == source.tobytes(), f"{label}: raw raster -2 pixels changed")
    image = layer.mask.topil(real=False, layer_sized=False)
    check(image is not None and image.tobytes() == stored and list(image.size) == raster["size"],
          f"{label}: decoded independent raster grid changed")
    return {"raw_header_bytes": len(raw["mask"]), "channel_id": -2,
            "sha256": digest(stored), "flags": flags, **parsed}


def edge_band(coverage):
    # AA tolerance is restricted to a one-pixel band around the independently
    # rasterized boundary. A shifted/missing contour cannot hide in a large
    # whole-image mean, and every remaining interior pixel is checked.
    padded = np.pad(coverage, 1, mode="edge")
    neighbors = [padded[y:y + coverage.shape[0], x:x + coverage.shape[1]]
                 for y in range(3) for x in range(3)]
    return (np.maximum.reduce(neighbors) - np.minimum.reduce(neighbors) > 1e-6) | ((coverage > 1e-6) & (coverage < 1 - 1e-6))


def compare_coverage(actual, expected, edge, label):
    delta = np.abs(actual.astype(np.float64) - expected.astype(np.float64))
    interior = delta[~edge]
    boundary = delta[edge]
    metrics = {"interior_pixels": int(interior.size), "boundary_pixels": int(boundary.size),
               "interior_max_byte_error": float(interior.max(initial=0)),
               "boundary_max_byte_error": float(boundary.max(initial=0)),
               "boundary_mean_byte_error": float(boundary.mean()) if boundary.size else 0.0,
               "whole_image_mean_byte_error": float(delta.mean())}
    check(interior.size > 0, f"{label}: no meaningful interior comparison")
    check(metrics["interior_max_byte_error"] <= 1, f"{label}: interior mismatch {metrics}")
    # Native uses 4x4 subpixel sampling; psd-tools integrates path pixel area.
    # Report AA separately rather than silently treating saved pixels as proof.
    check(metrics["boundary_max_byte_error"] <= 64 and metrics["boundary_mean_byte_error"] <= 16,
          f"{label}: excessive antialias difference {metrics}")
    return metrics


def density(coverage, byte):
    return np.floor(255.0 - (byte / 255.0) * (255.0 - coverage) + 0.5)


def fresh_geometry(folder, layer, entry, label):
    vector = entry["vector_mask"]
    viewport = tuple(vector["viewport"])
    # No PSDImage.composite(), layer.composite(), or saved alpha is used here.
    fresh = draw_vector_mask(layer, viewport=viewport)[:, :, 0].astype(np.float64)
    check(np.isfinite(fresh).all() and fresh.min() >= 0 and fresh.max() <= 1, f"{label}: fresh coverage range")
    edge = edge_band(fresh)
    native_sharp = reference(folder, vector["sharp_geometry"])
    check(native_sharp.shape == fresh.shape, f"{label}: fresh viewport dimensions")
    sharp_metrics = compare_coverage(fresh * 255, native_sharp, edge, label + " fresh sharp geometry")
    effective = np.floor(fresh * 255 + 0.5)
    if vector["inverted"]:
        effective = 255 - effective
    effective = density(effective, density_expectation(vector, label + " vector")["expected_byte"])
    if not vector["enabled"]:
        effective = np.full_like(effective, 255)
    effective_metrics = compare_coverage(effective, reference(folder, vector["effective_no_feather"]),
                                         edge, label + " explicit invert/density/disable")
    combined = effective.copy()
    raster = entry.get("raster_mask")
    if raster and raster["enabled"]:
        source = reference(folder, raster)
        yy, xx = np.indices(fresh.shape)
        xx = xx + viewport[0] - raster["bounds"][0]
        yy = yy + viewport[1] - raster["bounds"][1]
        inside = (xx >= 0) & (yy >= 0) & (xx < source.shape[1]) & (yy < source.shape[0])
        plane = np.full(fresh.shape, raster["fill"], dtype=np.float64)
        plane[inside] = source[yy[inside], xx[inside]]
        plane = density(plane, density_expectation(raster, label + " raster")["expected_byte"])
        combined = np.floor((combined * plane + 127) / 255)
    combined_metrics = compare_coverage(combined, reference(folder, vector["combined_no_feather"]),
                                       edge, label + " independent raster/vector multiplication")
    full = reference(folder, vector["native_full_inspection"])
    feather_changes_pixels = None
    if vector["feather"] is not None:
        feather_changes_pixels = bool(np.any(full != reference(folder, vector["effective_no_feather"])))
        check(feather_changes_pixels, f"{label}: native feather reference should exercise changed pixels")
    return {"renderer": "psd_tools.composite.vector.draw_vector_mask", "viewport": list(viewport),
            "sharp_geometry": sharp_metrics, "explicit_flags_density": effective_metrics,
            "independent_raster_product": combined_metrics,
            "native_feather_changes_pixels": feather_changes_pixels,
            "cross_renderer_feather_compared": False}


def verify_export(folder, manifest, export, *, transparent_group=False):
    psd = PSDImage.open(folder / export["file"])
    label = export["file"]
    write_report = export["write_report"]
    expected_rounding = sum(density_expectation(entry[field], entry["name"] + " " + field)["rounded"]
                            for entry in manifest["layers"] for field in ("raster_mask", "vector_mask")
                            if field in entry)
    check(write_report["appearance_fallback"] is None and write_report["baked_raster_masks"] is False,
          f"{label}: actual export fell back or baked independent masks")
    check(type(write_report["rounded_mask_densities"]) is int
          and write_report["rounded_mask_densities"] == expected_rounding,
          f"{label}: actual export rounding count disagrees with native f32 inputs")
    check(psd.version == export["version"] and list(psd.size) == manifest["canvas"] and psd.depth == 8,
          f"{label}: version/canvas/depth")
    raw = raw_layers(folder / export["file"], psd.version)
    all_layers = list(psd.descendants())
    layers = {layer.name: layer for layer in all_layers}
    check(len(layers) == len(all_layers) == len(manifest["layers"]), f"{label}: layer count or duplicate")
    check(set(layers) == {item["name"] for item in manifest["layers"]}, f"{label}: layer names")
    for order in manifest["sibling_order"]:
        container = psd if order["parent"] is None else layers[order["parent"]]
        check([layer.name for layer in container] == order["bottom_to_top"], f"{label}: sibling order")
    results = []
    for entry in manifest["layers"]:
        layer = layers[entry["name"]]
        context = f"{label}: {layer.name}"
        record = raw[layer.name]
        check(layer.is_group() == (entry["kind"] == "group"), f"{context}: kind")
        check((None if layer.parent is psd else layer.parent.name) == entry["parent"], f"{context}: parent")
        result = {"name": layer.name}
        if "source" in entry:
            source = reference(folder, entry["source"])
            top, left, bottom, right = record["bounds"]
            check([left, top, right, bottom] == entry["source"]["bounds"], f"{context}: raw source bounds")
            ids = [int(c.id) for c in layer._record.channel_info]
            for channel_id, index in ((0, 0), (1, 1), (2, 2), (-1, 3)):
                check(ids.count(channel_id) == 1, f"{context}: missing/duplicate source channel {channel_id}")
                pixels = layer._channels[ids.index(channel_id)].get_data(source.shape[1], source.shape[0], psd.depth, psd.version)
                check(pixels == source[:, :, index].tobytes(), f"{context}: source channel {channel_id} changed")
            decoded = layer.topil(apply_icc=False)
            check(decoded is not None and decoded.convert("RGBA").tobytes() == source.tobytes(),
                  f"{context}: decoded source RGBA changed")
            result["source_sha256"] = digest(source.tobytes())
        result["raster_mask"] = mask_record(folder, psd, layer, record, entry, context)
        if "vector_mask" in entry:
            result["vector_record"] = vector_record(layer, record, entry["vector_mask"], manifest["canvas"], context)
            result["fresh_geometry"] = fresh_geometry(folder, layer, entry, context)
        else:
            check(layer.vector_mask is None and not ({b"vmsk", b"vsms"} & record["tags"].keys()),
                  f"{context}: unexpected vector block")
        results.append(result)
    saved = reference(folder, manifest["composite"])
    stored_digest = digest(saved.tobytes())
    if transparent_group:
        check(manifest["schema"] == 2 and manifest["case_id"] == "original-root-14",
              "Transparent preview path is restricted to the schema-2 original group")
        from verify_psd_vector_packet import verify_transparent_group_preview
        stored_digest = verify_transparent_group_preview(folder / export["file"], psd, saved)
    else:
        check((saved[:, :, 3] == 255).all(), "Saved-composite reference must be opaque")
        preview = psd.topil(apply_icc=False)
        check(preview is not None and preview.convert("RGBA").tobytes() == saved.tobytes(), f"{label}: saved merged preview")
    return {"file": label, "version": psd.version, "layers": results, "actual_write_report": write_report,
            "saved_composite_checked_separately": True, "stored_composite_sha256": stored_digest}


def verify_native(folder, manifest):
    payloads = []
    for item in manifest["native_archives"]:
        path = folder / item["file"]
        check(digest(path.read_bytes()) == item["sha256"], f"{item['file']}: native archive digest")
        with zipfile.ZipFile(path) as archive:
            check(len(archive.namelist()) == len(set(archive.namelist())), "Duplicate native archive member")
            payloads.append({name: archive.read(name) for name in archive.namelist()})
    check(len(payloads) == 2 and payloads[0] == payloads[1], "PSD/PSB exports changed native archive payloads")
    check(manifest["native_document_unchanged"] is True, "Missing native-state assertion")
    native = json.loads(payloads[0]["emulsion.json"])
    check(manifest["blend_space"] == native["blend_space"] == "photoshop-srgb-v1" and native["version"] >= 14,
          "Native archive must preserve the explicit v14 PhotoshopSrgbV1 contract")
    check(any(name.startswith("emulsion/paths/") for name in payloads[0]), "Native archive lacks editable path resources")
    return {"before_after_members_identical": True, "member_count": len(payloads[0]),
            "native_blend_space": native["blend_space"], "native_version": native["version"],
            "members": {name: digest(value) for name, value in sorted(payloads[0].items())}}


def coverage_requirements(manifest):
    check(manifest["schema"] == 1 and manifest["kind"] == "editable-vector-mask-interchange", "Unknown manifest")
    check(manifest["canvas"] == [128, 80], "Golden packet requires the specified unequal canvas")
    check(sorted(item["version"] for item in manifest["exports"]) == [1, 2], "Both PSD and PSB required")
    descriptor_coverage(manifest)


def descriptor_coverage(manifest):
    entries = [item for item in manifest["layers"] if "vector_mask" in item]
    vectors = [item["vector_mask"] for item in entries]
    check(len(vectors) == 13, "Expected thirteen independently checked vector descriptors")
    for flag in ("enabled", "linked", "inverted"):
        check({v[flag] for v in vectors} == {False, True}, f"Missing {flag} coverage")
    check({v["initial_fill"] for v in vectors if not v["paths"]} == {0, 1}, "Missing empty hide/reveal")
    check(any(not p["closed"] for v in vectors for p in v["paths"]), "Missing implicit open-path closure")
    check(any(item["kind"] == "group" for item in entries), "Missing vector group")
    check(sum("raster_mask" in item for item in entries) == 3, "Missing independent raster carriers")
    check(any(v["density_byte"] == 153 and v["feather"] is None for v in vectors), "Missing separate density case")
    check(any(v["density_byte"] is None and v["feather"] == 2.25 for v in vectors), "Missing separate feather case")
    points = [a[field] for v in vectors for p in v["paths"] for a in p["anchors"] for field in ("preceding", "anchor", "leaving")]
    check(any(x < 0 or y < 0 for x, y in points) and any(x > 128 or y > 80 for x, y in points),
          "Missing negative and off-canvas coordinates")
    check(any(v["transform"][:4] != [1.0, 0.0, 0.0, 1.0] for v in vectors), "Missing baked affine geometry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    manifest = json.loads((args.directory / "manifest.json").read_text())
    if manifest.get("schema") == 2:
        from verify_psd_vector_packet import verify_packet
        report = verify_packet(args.directory, manifest)
    else:
        coverage_requirements(manifest)
        report = {"status": "passed", "reader": {"name": "psd-tools", "version": psd_tools.__version__, "module": psd_tools.__file__},
                  "scope": "Independent raw editable vector records, source pixels, raw raster masks, fresh path rasterization with explicit flags/density, unchanged native resources; no Adobe-application or cross-renderer feather parity",
                  "native_immutability": verify_native(args.directory, manifest),
                  "exports": [verify_export(args.directory, manifest, item) for item in manifest["exports"]]}
    output = json.dumps(report, indent=2) + "\n"
    if args.report:
        args.report.write_text(output)
    print(output, end="")


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, ValueError, KeyError, struct.error) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
