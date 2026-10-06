"""Schema-2 orchestration; original schema-1 record and coverage checks stay strict.

Invoked by verify_psd_vector_interchange.py, not a second user-facing command.
The authored cubic fallback and its separate 8.24 control are never conflated.
"""
from __future__ import annotations

import copy
import io
import json
import struct
import zipfile

import numpy as np
from PIL import Image
import psd_tools
from psd_tools import PSDImage

from verify_psd_vector_interchange import (
    Reader, SCALE, check, descriptor_coverage, digest, expected_packet, raw_layers,
    reference, rounded_fixed, verify_export, verify_native,
)


ORIGINAL_NAMES = {
    2: "Golden rectangle", 3: "Off-canvas cubic", 4: "Open affine contour",
    5: "Vector inverted", 6: "Vector unlinked", 7: "Vector disabled",
    8: "Empty reveal", 9: "Empty hide", 10: "Empty hide inverted",
    11: "Vector density only", 12: "Vector feather only",
    13: "Independent raster and vector", 14: "Editable vector group",
}
COMPLETE = "original-complete-scene"
CONTROL = "canonical-cubic-control"
CUBIC = "original-root-03"
GROUP = "original-root-14"


def verify_transparent_group_preview(path, psd, straight):
    """Check saved storage, not decoded straight color or fresh compositing.

    The current writer stores a white matte at fractional alpha: compute the
    separate f64 division, subtraction, products and sum in that order, then
    truncate to u8. Do not use integer algebra, round(), or an alpha tolerance.
    At alpha 0 or 255 the RGB bytes are unchanged, including hidden RGB.
    """
    r = Reader(path.read_bytes())
    check(r.take(4) == b"8BPS" and r.number("H") == psd.version, "Transparent header")
    check(r.take(6) == bytes(6) and r.number("H") == 4, "Transparent merged RGBA declaration")
    check([r.number("I"), r.number("I")] == [80, 128], "Transparent canvas")
    check(r.number("H") == 8 and r.number("H") == 3, "Transparent preview must be RGB8")
    r.block()
    r.block()
    lm = Reader(r.block("Q" if psd.version == 2 else "I"))
    info = Reader(lm.block("Q" if psd.version == 2 else "I"))
    check(info.number("h") < 0, "Fourth merged channel must be declared transparency")
    alpha = straight[:, :, 3]
    check(np.any((alpha > 0) & (alpha < 255)) and np.any(alpha == 0),
          "Group preview must exercise fractional and zero alpha")
    expected = straight.copy()
    for source, target in zip(straight.reshape(-1, 4), expected.reshape(-1, 4)):
        if 0 < int(source[3]) < 255:
            a = int(source[3]) / 255.0
            remainder = 255.0 * (1.0 - a)
            for channel in range(3):
                target[channel] = int(int(source[channel]) * a + remainder)
    planes = psd._record.image_data.get_data(psd._record.header)
    check(len(planes) == 4, "Exactly four saved merged planes required")
    for channel, actual in enumerate(planes):
        check(actual == expected[:, :, channel].tobytes(),
              f"Transparent saved white-matte plane {channel} differs")
    return digest(expected.tobytes())


def native_path(data, transform, origin):
    """Independently bind manifest world geometry to native lossless EMP1 bytes."""
    r = Reader(data)
    check(r.take(4) == b"EMP1", "Native editable path magic")

    def number(fmt):
        return struct.unpack("<" + fmt, r.take(struct.calcsize("<" + fmt)))[0]

    def point():
        x, y = struct.unpack("<dd", r.take(16))
        a, b, c, d, tx, ty = transform
        return [a * x + c * y + tx + origin[0], b * x + d * y + ty + origin[1]]

    paths = []
    for _ in range(number("I")):
        closed = number("B")
        check(closed in (0, 1), "Native closed flag")
        anchors = []
        for _ in range(number("I")):
            flags = number("B")
            check(flags & ~7 == 0, "Native anchor flags")
            anchor = point()
            preceding = point() if flags & 2 else anchor
            leaving = point() if flags & 4 else anchor
            anchors.append({"smooth": bool(flags & 1), "preceding": preceding,
                            "anchor": anchor, "leaving": leaving})
        paths.append({"closed": bool(closed), "anchors": anchors})
    check(r.pos == len(data), "Trailing native path data")
    return paths


def read_native(folder, manifest):
    result = verify_native(folder, manifest)
    with zipfile.ZipFile(folder / manifest["native_archives"][0]["file"]) as archive:
        members = {name: archive.read(name) for name in archive.namelist()}
    native = json.loads(members["emulsion.json"])
    check([native["width"], native["height"]] == manifest["canvas"] == [128, 80], "Native canvas binding")
    entries = {entry["name"]: entry for entry in manifest["layers"]}
    check(len(entries) == len(manifest["layers"]) == len(native["nodes"]), "Native inventory binding")
    ids = {node["id"]: node["name"] for node in native["nodes"]}
    check(len(ids) == len(native["nodes"]) and set(ids.values()) == set(entries), "Native unique node identity binding")
    parents = [None] + [node["id"] for node in native["nodes"] if node["kind"]["type"] == "group"]
    native_order = [{"parent": ids.get(parent),
                     "bottom_to_top": [node["name"] for node in native["nodes"] if node["parent"] == parent]}
                    for parent in parents]
    check(manifest["sibling_order"] == native_order, "Native sibling order binding")
    normalized = []
    for node in native["nodes"]:
        entry = entries[node["name"]]
        check(entry["parent"] == ids.get(node["parent"]), "Native parent binding")
        check(entry["kind"] == node["kind"]["type"], "Native kind binding")
        normalized_node = copy.deepcopy(node)
        kind = node["kind"]
        origin = [0, 0]
        if kind["type"] == "raster":
            origin = [kind["placement"]["x"], kind["placement"]["y"]]
            source = reference(folder, entry["source"])
            stored = Image.open(io.BytesIO(members[kind["src"]]))
            check(stored.mode == "RGBA" and stored.size == (kind["width"], kind["height"])
                  and stored.tobytes() == source.tobytes(), "Native source pixel binding")
            check(entry["source"]["bounds"] == [*origin, origin[0] + kind["width"], origin[1] + kind["height"]],
                  "Native source placement binding")
            normalized_node["kind"]["src"] = digest(members[kind["src"]])
        check((node["mask"] is not None) == ("raster_mask" in entry), "Native raster-mask binding")
        if node["mask"] is not None:
            stored = Image.open(io.BytesIO(members[node["mask"]]))
            mask = reference(folder, entry["raster_mask"])
            check(stored.mode == "L" and stored.size == (mask.shape[1], mask.shape[0])
                  and stored.tobytes() == mask.tobytes(), "Native raw-mask pixel binding")
            normalized_node["mask"] = digest(members[node["mask"]])
            expected = entry["raster_mask"]
            check(expected["enabled"] == node["mask_enabled"]
                  and expected["linked"] == node["mask_linked"]
                  and expected["fill"] == node["mask_fill"], "Native raster-mask flags binding")
            left = origin[0] + node["mask_transform"][4]
            top = origin[1] + node["mask_transform"][5]
            check(node["mask_transform"][:4] == [1, 0, 0, 1]
                  and expected["bounds"] == [left, top, left + mask.shape[1], top + mask.shape[0]],
                  "Native raster-mask placement binding")
            properties = node.get("mask_properties", {})
            check(expected["density_f32_bits"] == struct.unpack(">I", struct.pack(">f", properties.get("density", 1.0)))[0]
                  and (expected["feather"] or 0.0) == properties.get("feather", 0.0), "Native raster-mask properties binding")
        vector = node.get("vector_mask")
        check((vector is not None) == ("vector_mask" in entry), "Native vector presence binding")
        if vector is not None:
            expected = entry["vector_mask"]
            for field in ("enabled", "linked", "inverted", "transform"):
                check(vector[field] == expected[field], f"Native vector {field} binding")
            check(expected["initial_fill"] == int(vector["empty_coverage"] == "reveal_all"), "Native empty fill binding")
            check(expected["density_f32_bits"] == struct.unpack(">I", struct.pack(">f", vector["properties"]["density"]))[0]
                  and (expected["feather"] or 0.0) == vector["properties"]["feather"], "Native vector properties binding")
            check(expected["paths"] == native_path(members[vector["path"]], vector["transform"], origin),
                  "Native exact world geometry binding")
            normalized_node["vector_mask"]["path"] = digest(members[vector["path"]])
        normalized.append(normalized_node)
    return result, native, normalized


def case_requirements(manifest, case_id, names, expected_fallback):
    check(manifest["schema"] == 2 and manifest["kind"] == "vector-mask-interchange-case"
          and manifest["case_id"] == case_id, "Unknown scene manifest")
    check(manifest["origin"] == ("canonical-8.24-control" if case_id == CONTROL else "unchanged-original"),
          "Original/control provenance disclosure")
    check(manifest["expected_appearance_fallback"] == expected_fallback, "Expected fallback reason changed")
    check(sorted(export["version"] for export in manifest["exports"]) == [1, 2], "Both PSD and PSB required")
    check(len(manifest["layers"]) == len(names) and {entry["name"] for entry in manifest["layers"]} == names,
          "Required original case layers missing or substituted")


def verify_fallback(folder, manifest, export, reason):
    label = export["file"]
    psd = PSDImage.open(folder / label)
    check(psd.version == export["version"] and list(psd.size) == [128, 80] and psd.depth == 8,
          "Fallback format/canvas/depth")
    check(export["write_report"] == {"appearance_fallback": reason, "baked_raster_masks": False,
                                     "rounded_mask_densities": 0}, "Actual fallback report")
    layers = list(psd.descendants())
    check(len(layers) == 1 and not layers[0].is_group() and "flattened" in layers[0].name,
          "Fallback must contain one explicitly named appearance layer")
    raw = raw_layers(folder / label, psd.version)
    check(len(raw) == 1 and all(not ({b"vmsk", b"vsms"} & record["tags"].keys()) for record in raw.values()),
          "Fallback must not claim editable vector records")
    saved = reference(folder, manifest["composite"])
    check((saved[:, :, 3] == 255).all(), "Original fallback preview must remain opaque")
    preview = psd.topil(apply_icc=False)
    check(preview is not None and preview.convert("RGBA").tobytes() == saved.tobytes(), "Exact original fallback saved preview")
    layer = layers[0]
    record = raw[layer.name]
    check(record["bounds"] == (0, 0, 80, 128) and not record["mask"] and layer.vector_mask is None,
          "Fallback appearance layer bounds/masks")
    channel_ids = [int(channel.id) for channel in layer._record.channel_info]
    for channel_id, index in ((0, 0), (1, 1), (2, 2), (-1, 3)):
        check(channel_ids.count(channel_id) == 1, "Fallback source channels")
        pixels = layer._channels[channel_ids.index(channel_id)].get_data(128, 80, 8, psd.version)
        check(pixels == saved[:, :, index].tobytes(), "Exact original fallback appearance source")
    return {"file": label, "actual_write_report": export["write_report"],
            "editable_vector_records": 0, "original_preview_and_appearance_source_exact": True}


def difference(left, right):
    check(left.shape == right.shape, "Original/control reference shape")
    delta = np.abs(left.astype(np.int16) - right.astype(np.int16))
    return {"changed_samples": int(np.count_nonzero(delta)),
            "changed_pixels": int(np.count_nonzero(np.any(delta != 0, axis=2))) if delta.ndim == 3 else int(np.count_nonzero(delta)),
            "maximum_byte_difference": int(delta.max(initial=0))}


def cubic_control(folder, packet, cases):
    disclosure = packet["canonical_control"]
    check(disclosure["original_case"] == CUBIC and disclosure["control_case"] == CONTROL
          and disclosure["same_expected_8_24_words"] is True
          and disclosure["native_geometry_and_coverage_changed"] is True
          and bool(disclosure["disclosure"]), "Missing original/control disclosure")
    original, control = cases[CUBIC], cases[CONTROL]
    source = next(entry for entry in original["layers"] if entry["name"] == ORIGINAL_NAMES[3])
    canonical = next(entry for entry in control["layers"] if entry["name"] == ORIGINAL_NAMES[3])
    left, right = source["vector_mask"], canonical["vector_mask"]
    check(expected_packet(left, [128, 80]) == expected_packet(right, [128, 80]),
          "Canonical control changed expected standard path records")
    expected_words = []
    changed = [0, 0]
    max_delta = [0.0, 0.0]
    expected_paths = copy.deepcopy(left["paths"])
    for path in expected_paths:
        for anchor in path["anchors"]:
            for role in ("preceding", "anchor", "leaving"):
                point = anchor[role]
                words = [rounded_fixed(point[axis] / dimension) for axis, dimension in enumerate((128, 80))]
                expected_words.extend((words[1], words[0]))
                for axis, dimension in enumerate((128, 80)):
                    reconstructed = words[axis] / SCALE * dimension
                    delta = abs(point[axis] - reconstructed)
                    changed[axis] += int(delta != 0)
                    max_delta[axis] = max(max_delta[axis], delta)
                    point[axis] = reconstructed
    check(right["paths"] == expected_paths and right["transform"] == [1, 0, 0, 1, 6, 5],
          "Control must use exact document-space 8.24 reconstruction")
    check(changed == [0, 9] and max_delta == [0.0, 0.0000019073486328125],
          "Authored cubic coordinate differences changed")
    raw = (folder / disclosure["expected_words"]["file"]).read_bytes()
    check(digest(raw) == disclosure["expected_words"]["sha256"]
          and raw == struct.pack(">" + "i" * len(expected_words), *expected_words), "Expected signed 8.24 words")
    composite = difference(reference(folder / CUBIC, original["composite"]), reference(folder / CONTROL, control["composite"]))
    coverage = difference(reference(folder / CUBIC, left["effective_no_feather"]), reference(folder / CONTROL, right["effective_no_feather"]))
    check(composite["changed_pixels"] > 0 and coverage["changed_pixels"] > 0, "Control must disclose changed native appearance and coverage")
    return {"same_expected_path_record_bytes": True, "same_expected_signed_words": True,
            "changed_world_coordinates_xy": changed, "maximum_world_delta_xy": max_delta,
            "native_composite_difference": composite, "native_coverage_difference": coverage,
            "original_native_appearance_is_editable": False,
            "disclosure": disclosure["disclosure"]}


def verify_packet(folder, packet):
    check(packet["schema"] == 2 and packet["kind"] == "vector-mask-interchange-packet", "Unknown packet")
    expected_ids = {COMPLETE, CONTROL} | {f"original-root-{index:02}" for index in ORIGINAL_NAMES}
    index = {entry["id"]: entry for entry in packet["cases"]}
    check(len(index) == len(packet["cases"]) == 15 and set(index) == expected_ids, "Exact 15-case packet inventory required")
    check(packet["original_source_case"] == COMPLETE
          and packet["original_vector_names"] == list(ORIGINAL_NAMES.values()), "Original thirteen-case inventory")
    cases = {}
    for case_id, entry in index.items():
        check(entry["manifest"] == f"{case_id}/manifest.json", "Case path binding")
        cases[case_id] = json.loads((folder / entry["manifest"]).read_text())
    all_names = set(ORIGINAL_NAMES.values()) | {"Opaque background", "Group source"}
    original_case = cases[COMPLETE]
    case_requirements(original_case, COMPLETE, all_names, "UnsupportedFeatures")
    descriptor_coverage(original_case)
    _, original, original_nodes = read_native(folder / COMPLETE, original_case)
    check([node["name"] for node in original_nodes if node.get("vector_mask")] == list(ORIGINAL_NAMES.values()),
          "Original native vector inventory")
    results = []
    for case_id, manifest in cases.items():
        expected_fallback = "UnsupportedFeatures" if case_id == COMPLETE else "BlendSpaceDifference" if case_id == CUBIC else None
        names = all_names if case_id == COMPLETE else {"Editable vector group", "Group source"} if case_id == GROUP else {
            "Opaque background", ORIGINAL_NAMES[3 if case_id == CONTROL else int(case_id.rsplit("-", 1)[1])]}
        case_requirements(manifest, case_id, names, expected_fallback)
        native_result, native, nodes = read_native(folder / case_id, manifest)
        check({key: value for key, value in native.items() if key != "nodes"}
              == {key: value for key, value in original.items() if key != "nodes"}, "Original document metadata changed")
        expected_nodes = [node for node in original_nodes if node["name"] in names]
        compared_nodes = copy.deepcopy(nodes)
        if case_id == CONTROL:
            for node, expected_node in zip(compared_nodes, expected_nodes):
                if node["name"] == ORIGINAL_NAMES[3]:
                    check(node["vector_mask"]["path"] != expected_node["vector_mask"]["path"], "Canonical path must remain separate")
                    node["vector_mask"]["path"] = expected_node["vector_mask"]["path"]
                    node["vector_mask"]["transform"] = expected_node["vector_mask"]["transform"]
        check(compared_nodes == expected_nodes, "Extracted original descriptors/resources/order changed")
        expected_order = [{"parent": item["parent"], "bottom_to_top": [name for name in item["bottom_to_top"] if name in names]}
                          for item in original_case["sibling_order"] if item["parent"] is None or item["parent"] in names]
        check(manifest["sibling_order"] == expected_order, "Extracted original sibling order changed")
        export_results = []
        for export in manifest["exports"]:
            check(digest((folder / case_id / export["file"]).read_bytes()) == export["sha256"], "Actual export digest")
            saved = reference(folder / case_id, manifest["composite"])
            reopened = reference(folder / case_id, export["reopened_composite"])
            check(np.array_equal(saved, reopened), "Exact original/control native reopen appearance")
            vector_count = 0 if expected_fallback else 1
            check(export["reopened_vector_count"] == vector_count, "Actual native reopen vector count")
            report = verify_fallback(folder / case_id, manifest, export, expected_fallback) if expected_fallback else verify_export(
                folder / case_id, manifest, export, transparent_group=case_id == GROUP)
            report["native_layered_reopen_appearance_exact"] = expected_fallback is None
            report["native_fallback_reopen_appearance_exact"] = expected_fallback is not None
            report["saved_preview_representation"] = "exact white-matted RGB plus straight alpha planes" if case_id == GROUP else "exact opaque native RGBA"
            export_results.append(report)
        results.append({"case_id": case_id, "origin": manifest["origin"], "native_immutability": native_result,
                        "original_source_binding": True, "exports": export_results})
    check(packet["density_packet"] == "density-boundaries/manifest.json", "Separate density packet required")
    # Use the unchanged six-case verifier, including its mandatory byte/metadata
    # negative controls. This makes the complete packet fail if density fails.
    from verify_psd_density_interchange import verify as verify_density
    return {"status": "passed", "schema": 2,
            "reader": {"name": "psd-tools", "version": psd_tools.__version__, "module": psd_tools.__file__},
            "original_vector_cases": 13, "original_editable_vector_cases": 12,
            "separate_canonical_vector_controls": 1, "original_fallback_scenes": 2,
            "scope": "Original sources retained; admitted editable records and explicit exact fallbacks. No Adobe-application or feather-kernel parity.",
            "cases": results, "canonical_control": cubic_control(folder, packet, cases),
            "density_packet": verify_density(folder / "density-boundaries")}
