#!/usr/bin/env python3
"""Extract four bounded mask-mixer arrays from two pinned PSD captures.

Requires existing psd-tools 1.23.0, Pillow and numpy. No download, authoring editor,
Gaussian/resize implementation, or product codec is invoked. Array bytes come
only from independently read source PSD data. The FEid cache is NOT the original
editable PNG source. Keep the upstream MIT license with redistributed inputs
or derived arrays. The PSDs are explicitly untagged, not certified sRGB.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import struct

import numpy as np
from psd_tools import PSDImage
from psd_tools.constants import Tag, Resource
from psd_tools.compression import decompress

PIN = '20f95a201c395213ce3e212f13d912e418d1cba6'
BASE = 'photoshop-smart-filter-instances-base.psd'
RASTER = 'photoshop-smart-filter-instances-rasterized.psd'
FILE_HASHES = {
    BASE: '53b66e9a80970529c3791896a4ce7a01e219c158df001bdc35bdeadf9ca47c85',
    RASTER: '0db3fa1efb306596a5c2683921b9ca49780ad7c95582ef7c14feb76115e6a4d0',
}
ARRAY_HASHES = {
    'projected-unfiltered-rgba8': '0996e51e6f67b58f1413f18d3bed03dd2d7f2df37c73ff0008202510ccea7d8f',
    'unmasked-filtered-rgba8': 'fb6598f521a19724e11ed69f8aaadf1c02096945b5182ea9dc3c9b2fb681ae88',
    'shared-mask-u8': 'a62ff6d17156635bfc8ba0319bc5f27cc9d19a9ff28a53b46d085c1a8b6c6000',
    'photoshop-rasterized-target-rgba8': '17a9045e38ffa50c8702014e56af16fa4d2f5dac12d9d04d3186265f25eead6b',
}

def sha(data):
    return hashlib.sha256(data).hexdigest()

def document_rgba(layer):
    out = np.zeros((40, 40, 4), np.uint8)
    left, top, right, bottom = layer.bbox
    assert 0 <= left < right <= 40 and 0 <= top < bottom <= 40
    image = layer.topil(apply_icc=False).convert('RGBA')
    out[top:bottom, left:right] = np.asarray(image)
    return out

def decode_channel(channel):
    assert channel.compression == 1
    # FEid's row counts are 32-bit even though these outer PSDs are version 1.
    rows = struct.unpack('>40I', channel.data[:160])
    assert sum(rows) == len(channel.data) - 160
    data = decompress(channel.data, channel.compression, 40, 40, 8, 2)
    assert len(data) == 1600
    return np.frombuffer(data, np.uint8).reshape(40, 40)

def extract(inputs, output):
    files = {}
    for name, expected in FILE_HASHES.items():
        path = inputs / name
        assert sha(path.read_bytes()) == expected, f'{name}: pinned hash mismatch'
        files[name] = PSDImage.open(path)
        assert files[name].size == (40, 40) and files[name].depth == 8
        assert files[name].version == 1
        assert not files[name].has_preview()
        assert files[name].image_resources.get_data(Resource.ICC_PROFILE) is None
        assert files[name].image_resources.get_data(Resource.ICC_UNTAGGED_PROFILE) == 1
    base, raster = files[BASE], files[RASTER]
    layer = next(l for l in base if l.name == 'Instance B Gaussian 4.5 transformed masked')
    target = next(l for l in raster if l.name == 'Instance B rasterized from filtered object')
    assert not target.has_mask() and Tag.SMART_OBJECT_LAYER_DATA1 not in target.tagged_blocks
    descriptor = layer.tagged_blocks.get_data(Tag.SMART_OBJECT_LAYER_DATA1).data
    placed = descriptor[b'placed'].value.rstrip('\0')
    source = descriptor[b'Idnt'].value.rstrip('\0')
    assert placed == 'efc62d75-9b6e-174a-a70c-f543171a472b' and placed != source
    effects = base.tagged_blocks.get_data(Tag.FILTER_EFFECTS2)
    selected = [e for e in effects if e.uuid == placed]
    assert len(selected) == 1
    effect = selected[0]
    assert effects.version == 3 and effect.version == 1
    assert effect.rectangle == (0, 0, 40, 40) and effect.depth == 8
    assert effect.max_channels == 24 and list(effect.extra.rectangle) == [0, 0, 40, 40]
    assert [i for i, c in enumerate(effect.channels) if c.is_written] == [0, 1, 2, 25]
    arrays = {
        'projected-unfiltered-rgba8': np.stack([decode_channel(effect.channels[i]) for i in (0, 1, 2, 25)], -1),
        'unmasked-filtered-rgba8': document_rgba(layer),
        'shared-mask-u8': decode_channel(effect.extra),
        'photoshop-rasterized-target-rgba8': document_rgba(target),
    }
    assert np.array_equal(document_rgba(base[0]), document_rgba(raster[0]))
    output.mkdir(parents=True, exist_ok=True)
    manifest = {'commit': PIN, 'source_files': [], 'arrays': []}
    for name, expected in FILE_HASHES.items():
        manifest['source_files'].append({'file':name, 'sha256':expected, 'url':f'https://github.com/SethRobinson/Patchy/blob/{PIN}/test-fixtures/psd/{name}'})
    for name, array in arrays.items():
        raw = array.tobytes()
        assert sha(raw) == ARRAY_HASHES[name], f'{name}: independent decode changed'
        path = output / (name + '.bin')
        path.write_bytes(raw)
        manifest['arrays'].append({'file':path.name, 'shape':list(array.shape), 'dtype':'uint8', 'order':'row-major, channel-interleaved', 'document_origin_xy':[0,0], 'sha256':sha(raw), 'bytes':len(raw)})
    manifest['license'] = {'url':f'https://github.com/SethRobinson/Patchy/blob/{PIN}/LICENSE', 'sha256':'bbc50c8c376e0e5980939be7df6769feed1a30289c7efc6391b204dfb15de88d', 'notice':'MIT; copyright 2026 Seth A. Robinson'}
    manifest['authorship_commit'] = 'https://github.com/SethRobinson/Patchy/commit/52091ed6681326e93dae41fd7c4bafc3b3c2832e'
    manifest['scope'] = 'Stored-encoded-RGB shared-mask interpolation only. Untagged documents. No source-colorimetry, Gaussian, scale, editable-source, linked-mask, density/feather, or newly authored reference-editor acceptance claim.'
    (output / 'extraction-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Verified and extracted {len(arrays)} arrays ({sum(a.nbytes for a in arrays.values())} bytes).')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inputs', type=Path, required=True, help='Directory containing the two pinned PSDs')
    parser.add_argument('--output', type=Path, required=True, help='Directory for four raw arrays and metadata')
    args = parser.parse_args()
    extract(args.inputs, args.output)
