#!/usr/bin/env python3
"""Verify/re-extract bounded PSD blending references; never composite.

Requires psd-tools 1.23.0 and Pillow. Run with --write only to regenerate the
manifest and saved-image PNGs, or --write-manifest to update metadata only.
Inputs must already exist at their pinned hashes.
This deliberately calls neither PSDImage.composite nor any native renderer.
"""

import argparse
import hashlib
import json
import struct
from pathlib import Path

import psd_tools
from PIL import Image
from psd_tools import PSDImage
from psd_tools.constants import ColorMode, Resource


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/emulsion-io/tests/fixtures/psd/blending"
REVISION = "d68bf46c7140a1f8c74be9c10b4e21103e820761"
SOURCES = {
    "opacity-fill": (
        "opacity-fill.psd",
        "b3568a8c6491d2da4b9e2aa8c6842b443c09b51aca6ffe44bcbfcc2ae17189ef",
    ),
    "group-clipping": (
        "group-clipping/group-clipping.psd",
        "b8e346ee1909c793c5d1be36b01dacad7152d3f40757b4271cfb869d4a5ba141",
    ),
    "knockout-none-nested": (
        "transparency/knockout-none-nested.psd",
        "9755c4adda49116b7afac2280e9a481ec1420e65ecbd3de54736df0fb902aaf5",
    ),
    "knockout-shallow-nested": (
        "transparency/knockout-shallow-nested.psd",
        "b94230c312c3caea229e13a27264fd9f89f719fa98de0cf87f539cfb8ab25dd6",
    ),
    "knockout-deep-nested": (
        "transparency/knockout-deep-nested.psd",
        "d206355323809135408357a715814112852fe52d55269a9e5feb681567dff6f8",
    ),
    "knockout-deep-nested-pt": (
        "transparency/knockout-deep-nested-pt.psd",
        "0cc7495b55f1b937f9d1c465e6d736f58233fbfbbea69b7eadefb49ac831ef8c",
    ),
}
PHOTOSHOP_PNG_HASH = "ba0190dfa96f17dfa6a4f9ebdc608682c54df222adc01049f66143614fd46cef"
LICENSE_HASH = "79f7e019a30b97542cce6f6482a75ba26771b4204c374fec305dc8cf558d137f"
FLAGS = {b"infx": True, b"clbl": True, b"tsly": True, b"lmgm": False}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verify_hash(path, expected):
    data = path.read_bytes()
    assert digest(data) == expected, f"Unrecognized input: {path}"
    return data


def raw_layer_roles(data):
    """Read record-scoped role evidence directly, without decoding layer pixels.

    This is deliberately bounded to the six pinned PSD (not PSB) RGB8 sources.
    Tagged blocks are visited only inside each record's length-delimited extra
    data. Neither a layer name nor a global search for lnsr can establish a role.
    """
    assert data[:6] == b"8BPS\x00\x01"
    assert struct.unpack_from(">HH", data, 22) == (8, 3)
    position = 26

    def take(size, end):
        nonlocal position
        assert 0 <= size <= end - position, "Truncated PSD record"
        value = data[position:position + size]
        position += size
        return value

    def number(fmt, end):
        return struct.unpack(fmt, take(struct.calcsize(fmt), end))[0]

    for _ in range(2):  # Color-mode data and image resources.
        take(number(">I", len(data)), len(data))
    section_size = number(">I", len(data))
    section_end = position + section_size
    assert section_end <= len(data)
    info_size = number(">I", section_end)
    info_end = position + info_size
    assert info_end <= section_end
    count = abs(number(">h", info_end))
    result = []
    for index in range(count):
        top, left, bottom, right = struct.unpack(">iiii", take(16, info_end))
        channels = []
        for _ in range(number(">H", info_end)):
            channels.append(number(">h", info_end))
            take(4, info_end)  # Channel-data length, not channel data itself.
        assert take(4, info_end) == b"8BIM"
        take(6, info_end)  # Blend mode, opacity, and clipping.
        flags = number(">B", info_end)
        take(1, info_end)
        extra_size = number(">I", info_end)
        extra_end = position + extra_size
        assert extra_end <= info_end
        for _ in range(2):  # Raster-mask metadata and Blend If ranges.
            take(number(">I", extra_end), extra_end)
        name_size = number(">B", extra_end) + 1
        take(((name_size + 3) // 4) * 4 - 1, extra_end)
        tags = {}
        while extra_end - position >= 12:
            assert take(4, extra_end) == b"8BIM"
            key = take(4, extra_end)
            payload = take(number(">I", extra_end), extra_end)
            if key in (b"lspf", b"lnsr"):
                assert key not in tags and len(payload) == 4
                tags[key] = payload
        assert all(value == 0 for value in take(extra_end - position, extra_end))
        result.append({
            "record_index": index,
            "channel_ids": channels,
            "flags_byte": flags,
            "lspf": int.from_bytes(tags[b"lspf"], "big") if b"lspf" in tags else None,
            "lnsr": tags[b"lnsr"].decode("ascii") if b"lnsr" in tags else None,
            "bounds": [left, top, right, bottom],
        })
    return result


def role_metadata(layer, psd, raw_records, record_indices):
    record = layer._record
    evidence = raw_records[record_indices[id(record)]].copy()
    assert evidence.pop("bounds") == [record.left, record.top, record.right, record.bottom]
    assert evidence["channel_ids"] == [int(channel.id) for channel in record.channel_info]
    assert evidence["flags_byte"] == record.flags.tobytes()[0]
    lspf = layer.tagged_blocks.get_data(b"lspf")
    lnsr = layer.tagged_blocks.get_data(b"lnsr")
    assert evidence["lspf"] == (int(lspf) if lspf is not None else None)
    assert evidence["lnsr"] == (lnsr.decode("ascii") if lnsr is not None else None)
    evidence["photoshop_background"] = (
        layer.parent is psd and psd[0] is layer and layer.kind == "pixel"
        and not bool(record.clipping)
        and evidence["channel_ids"] == [0, 1, 2]
        and evidence["flags_byte"] == 0x09
        and evidence["lspf"] == 0x0D and evidence["lnsr"] == "bgnd"
    )
    return evidence


def pixel_spec(layer):
    """Losslessly reduce raw layer channels to a constant and optional alpha hole."""
    image = layer.topil(apply_icc=False).convert("RGBA")
    colors = image.getcolors(maxcolors=3)
    assert colors and len(colors) <= 2, (layer.name, "not a controlled color field")
    opaque = [rgba for _, rgba in colors if rgba[3] == 255]
    assert len(opaque) == 1
    rgba = opaque[0]
    assert all(color[:3] == rgba[:3] and color[3] in (0, 255) for _, color in colors)
    hole = image.getchannel("A").point(lambda value: 255 - value).getbbox()
    reconstructed = Image.new("RGBA", image.size, rgba)
    if hole:
        reconstructed.paste((*rgba[:3], 0), hole)
    assert reconstructed.tobytes() == image.tobytes(), (layer.name, "nonrectangular alpha")
    return {
        "rgba": list(rgba),
        "transparent_rect": list(hole) if hole else None,
        "raw_rgba_sha256": digest(image.tobytes()),
    }


def extract_node(layer, psd, raw_records, record_indices):
    assert layer.kind in ("pixel", "solidcolorfill", "group")
    assert layer.visible and not layer.has_vector_mask() and len(layer.effects) == 0
    assert layer.blend_mode.value in (b"norm", b"pass")
    ranges = layer._record.blending_ranges
    neutral = [(0, 65535), (0, 65535)]
    assert not ranges.composite_ranges or ranges.composite_ranges == neutral
    assert all(r == neutral for r in ranges.channel_ranges)
    # opacity-fill has an empty all-white mask, equivalent to no coverage gate.
    mask = None
    if layer.has_mask():
        assert layer.mask.bbox == (0, 0, 0, 0)
        assert layer.mask.background_color == 255 and layer.mask.topil() is None
        assert layer.mask._data.parameters is None
        mask = "empty-all-white"
    tagged = layer.tagged_blocks
    raw_clip = bool(layer._record.clipping)
    group = layer.is_group()
    # This compatibility exception is independently demonstrated by the separate
    # upstream reference PNG export, NOT inferred from the PSD's different merged data.
    effective_clip = raw_clip and not group
    result = {
        "name": layer.name,
        "kind": "group" if group else "pixels",
        "bounds": list(layer.bbox),
        "opacity": layer.opacity,
        "fill_opacity": layer.fill_opacity,
        "blend": layer.blend_mode.value.decode("ascii"),
        "knockout": int(tagged.get_data(b"knko", 0)),
        "stored_clipping": raw_clip,
        "photoshop_effective_clipping": effective_clip,
        "flags": {key.decode(): bool(tagged.get_data(key, default)) for key, default in FLAGS.items()},
        "raw_role": role_metadata(layer, psd, raw_records, record_indices),
        "neutral_mask": mask,
        "pixels": None if group else pixel_spec(layer),
        "children": [extract_node(child, psd, raw_records, record_indices) for child in layer] if group else [],
    }
    if layer.kind == "solidcolorfill":
        color = tagged.get_data(b"SoCo")[b"Clr "]
        assert [float(color[c].value) for c in (b"Rd  ", b"Grn ", b"Bl  ")] == [255, 0, 0]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--write", action="store_true", help="regenerate manifest and extracted saved PNGs")
    output.add_argument("--write-manifest", action="store_true", help="regenerate metadata without writing any PNG")
    args = parser.parse_args()
    assert psd_tools.__version__ == "1.23.0", psd_tools.__version__
    verify_hash(FIXTURES.parent / "LICENSE.psd-tools", LICENSE_HASH)
    original_png = FIXTURES / "group-clipping-photoshop.png"
    verify_hash(original_png, PHOTOSHOP_PNG_HASH)
    cases = []
    for name, (upstream_path, sha256) in SOURCES.items():
        path = FIXTURES / f"{name}.psd"
        data = verify_hash(path, sha256)
        raw_records = raw_layer_roles(data)
        psd = PSDImage.open(path)
        records = psd._record.layer_and_mask_information.layer_info.layer_records
        assert len(raw_records) == len(records)
        record_indices = {id(record): index for index, record in enumerate(records)}
        assert psd.depth == 8 and psd.color_mode == ColorMode.RGB
        assert psd.size == ((627, 510) if name == "group-clipping" else (32, 32))
        version = psd.image_resources.get_data(Resource.VERSION_INFO)
        if name != "group-clipping":
            assert version.writer == "Adobe Photoshop"
            assert version.reader == ("Adobe Photoshop CC 2019" if name == "opacity-fill" else "Adobe Photoshop 2026")
        saved = psd.topil(apply_icc=False).convert("RGBA")
        if name == "group-clipping":
            expected = Image.open(original_png).convert("RGBA")
            assert saved.tobytes() != expected.tobytes()
            expected_file = original_png.name
        else:
            expected = saved
            expected_file = f"{name}-photoshop.png"
            known = {
                "opacity-fill": (255, 0, 0, 115),
                "knockout-none-nested": (0, 127, 128, 255),
                "knockout-shallow-nested": (127, 0, 128, 255),
                "knockout-deep-nested": (127, 0, 128, 255),
                "knockout-deep-nested-pt": (127, 127, 255, 255),
            }[name]
            assert expected.getcolors() == [(1024, known)]
            if args.write:
                expected.save(FIXTURES / expected_file)
            else:
                actual = Image.open(FIXTURES / expected_file).convert("RGBA")
                assert actual.size == expected.size and actual.tobytes() == expected.tobytes()
        nodes = [extract_node(layer, psd, raw_records, record_indices) for layer in psd]
        targets = [index for index, node in enumerate(nodes) if node["raw_role"]["photoshop_background"]]
        assert targets == ([0] if name.startswith("knockout-") else [])
        if targets:
            ordinary = nodes[1]["raw_role"]
            assert ordinary["channel_ids"] == [-1, 0, 1, 2]
            assert ordinary["flags_byte"] == 0x08 and ordinary["lspf"] == 0
            assert ordinary["lnsr"] is None and not ordinary["photoshop_background"]
        cases.append({
            "name": name,
            "size": list(psd.size),
            "source_path": upstream_path,
            "source_sha256": sha256,
            "writer": version.writer if version else None,
            "reader": version.reader if version else None,
            "expected_png": expected_file,
            "expected_origin": "separate-upstream-photoshop-export" if name == "group-clipping" else "photoshop-saved-merged-channels",
            "expected_rgba_sha256": digest(expected.tobytes()),
            "saved_merged_rgba_sha256": digest(saved.tobytes()),
            "knockout_background_root_index": targets[0] if targets else None,
            "nodes": nodes,
        })
    manifest = {"schema": 2, "upstream_revision": REVISION, "psd_tools": "1.23.0", "apply_icc": False, "cases": cases}
    text = json.dumps(manifest, indent=2) + "\n"
    manifest_path = FIXTURES / "manifest.json"
    if args.write or args.write_manifest:
        manifest_path.write_text(text)
    else:
        assert manifest_path.read_text() == text, "Manifest differs from independently decoded sources"
    print(f"Verified {len(cases)} pinned PSDs, record-scoped Background roles, raw numeric layer inputs, and independent expected RGBA pixels.")


if __name__ == "__main__":
    main()
