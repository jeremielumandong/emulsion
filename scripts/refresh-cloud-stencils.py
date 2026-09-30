#!/usr/bin/env python3
"""Rebuild the offline AWS and Azure icon archive from the official icon downloads.

Usage: refresh-cloud-stencils.py <AWS icon folder> <Azure icon folder>
"""
import gzip
import json
import re
import sys
from pathlib import Path

output = Path(__file__).resolve().parent.parent / "assets/diagram-stencils/cloud-icons.json.gz"
aws, azure = (Path(p) for p in sys.argv[1:3])

AWS_CATEGORIES = {
    "IoT": "Internet-of-Things",
    "Management-Governance": "Management-Tools",
}
AWS_TITLES = {
    "Internet-of-Things": "Internet of Things",
    "Management-Tools": "Management & Governance",
    "Networking-Content-Delivery": "Networking & Content Delivery",
    "Security-Identity": "Security, Identity & Compliance",
    "Front-End-Web-Mobile": "Front-End Web & Mobile",
    "Migration-Modernization": "Migration & Modernization",
}
AZURE_WORDS = {"ai": "AI", "iot": "IoT", "devops": "DevOps", "intune": "Intune"}


def svg(path):
    text = path.read_text(encoding="utf-8")
    text = re.sub(r"<\?xml[^>]*\?>|<!--.*?-->|<title>.*?</title>", "", text, flags=re.S)
    return re.sub(r">\s+<", "><", text).strip()


def slug(text):
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def label(stem):
    return re.sub(r"\s+", " ", stem.replace("-", " ").replace("_", " ")).strip()


def files(folder):
    return sorted(p for p in folder.glob("*.svg") if "__MACOSX" not in p.parts)


packs = {}


def add(pack_id, name, provider, entry, path):
    pack = packs.setdefault(pack_id, {"id": pack_id, "name": name, "provider": provider, "entries": {}})
    pack["entries"].setdefault(entry, svg(path))


services = next(aws.glob("Architecture-Service-Icons_*"))
for folder in sorted(services.glob("Arch_*")):
    category = folder.name.removeprefix("Arch_")
    title = AWS_TITLES.get(category, label(category))
    for path in files(folder / "64"):
        if "_Dark" in path.stem:
            continue
        entry = label(re.sub(r"^Arch_|(_Light)?_64$", "", path.stem))
        add(f"aws-{slug(category)}", f"AWS {title}", "AWS", entry, path)
resources = next(aws.glob("Resource-Icons_*"))
for folder in sorted(resources.glob("Res_*")):
    category = folder.name.removeprefix("Res_")
    category = AWS_CATEGORIES.get(category, category)
    title = AWS_TITLES.get(category, label(category))
    for path in files(folder):
        if path.stem.endswith("_Dark"):
            continue
        entry = label(re.sub(r"^Res_|_48(_Light)?$", "", path.stem))
        add(f"aws-{slug(category)}", f"AWS {title}", "AWS", entry, path)
for path in files(next(aws.glob("Architecture-Group-Icons_*"))):
    if not path.stem.endswith("_Dark"):
        add("aws-groups", "AWS Groups", "AWS", label(re.sub(r"_32$", "", path.stem)), path)
for path in files(next(aws.glob("Category-Icons_*")) / "Arch-Category_64"):
    entry = label(re.sub(r"^Arch-Category_|_64$", "", path.stem))
    add("aws-categories", "AWS Categories", "AWS", entry, path)

icons = next(azure.rglob("Icons"))
for folder in sorted(p for p in icons.iterdir() if p.is_dir()):
    title = " ".join(AZURE_WORDS.get(w, w if w == "+" else w.capitalize()) for w in folder.name.split())
    for path in files(folder):
        entry = label(re.sub(r"^\d+-icon-service-", "", path.stem))
        add(f"azure-{slug(folder.name)}", title if title.startswith("Azure") else f"Azure {title}", "Azure", entry, path)

archive = [
    {**pack, "entries": [{"name": k, "svg": v} for k, v in pack["entries"].items()]}
    for pack in sorted(packs.values(), key=lambda p: (p["provider"], p["name"]))
]
encoded = json.dumps(archive, ensure_ascii=False, separators=(",", ":")).encode()
output.with_suffix(".tmp").write_bytes(gzip.compress(encoded, compresslevel=9, mtime=0))
output.with_suffix(".tmp").replace(output)
print(f"Archived {sum(len(p['entries']) for p in archive)} icons in {len(archive)} packs")
