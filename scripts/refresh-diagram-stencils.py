#!/usr/bin/env python3
"""Rebuild the pinned offline stencil archive, verifying every upstream input."""
import concurrent.futures
import gzip
import hashlib
import json
from pathlib import Path
import urllib.request
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parent.parent / "assets/diagram-stencils"
manifest = json.loads((root / "UPSTREAM.json").read_text())
base = f"https://raw.githubusercontent.com/jgraph/drawio/{manifest['commit']}/"

def fetch(record):
    with urllib.request.urlopen(base + record["path"], timeout=30) as response:
        data = response.read(8 * 1024 * 1024 + 1)
    if hashlib.sha256(data).hexdigest() != record["sha256"]:
        raise ValueError(f"Checksum mismatch: {record['path']}")
    library = ET.fromstring(data)
    namespace = library.attrib.get("name", "")
    return {
        namespace + "." + shape.attrib["name"].lower().replace(" ", "_"):
        ET.tostring(shape, encoding="unicode")
        for shape in library.findall("shape")
    }

with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
    catalog = {}
    for shapes in pool.map(fetch, manifest["files"]):
        catalog.update(shapes)
encoded = json.dumps(catalog, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
output = root / "drawio.json.gz"
output.with_suffix(".tmp").write_bytes(gzip.compress(encoded, mtime=0))
output.with_suffix(".tmp").replace(output)
print(f"Archived {len(catalog)} pinned stencil definitions")
