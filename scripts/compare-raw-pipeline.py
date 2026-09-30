#!/usr/bin/env python3
"""Compare RAW benchmark directories, requiring bit-identical developed pixels."""
import argparse
import json
from pathlib import Path
from statistics import median


def read(folder):
    photos = {}
    for path in folder.glob("*.json"):
        data = json.loads(path.read_text())
        if "edits" in data:
            photos.setdefault(data["file"], []).append(data)
    return photos


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    before, after = read(args.baseline), read(args.candidate)
    if not before or before.keys() != after.keys():
        raise SystemExit("photo sets differ or are empty")
    rows = []
    for file in sorted(before):
        reference = before[file][0]
        for run in before[file] + after[file]:
            for key in ["source_sha256", "dimensions", "preview_dimensions",
                        "cold_pixel_sha256", "full_pixel_sha256"]:
                assert run[key] == reference[key], (file, key)
            assert run["original_unchanged"]
            assert [(e["case"], e["pixel_sha256"]) for e in run["edits"]] == [
                (e["case"], e["pixel_sha256"]) for e in reference["edits"]], file
        row = {"file": file, "runs": [len(before[file]), len(after[file])]}
        for stage in ["load_ms", "cold_fit_ms", "full_develop_ms"]:
            a, b = (median(r[stage] for r in runs) for runs in [before[file], after[file]])
            row[stage] = {"before": a, "after": b, "reduction_pct": (1 - b / a) * 100}
        a, b = (median(r["load_ms"] + r["cold_fit_ms"] for r in runs)
                for runs in [before[file], after[file]])
        row["load_and_fit_ms"] = {"before": a, "after": b, "reduction_pct": (1 - b / a) * 100}
        row["edits"] = {}
        for i, case in enumerate(reference["edits"]):
            a, b = (median(v for r in runs for v in r["edits"][i]["develop_ms"])
                    for runs in [before[file], after[file]])
            row["edits"][case["case"]] = {"before": a, "after": b, "reduction_pct": (1 - b / a) * 100}
        rows.append(row)
    aggregate = {}
    for group in ["all", "LandscapeRaw", "PortraitRaw"]:
        selected = rows if group == "all" else [r for r in rows if r["file"].startswith(group + "/")]
        if not selected:
            continue
        aggregate[group] = {"files": len(selected)}
        for stage in ["load_ms", "cold_fit_ms", "full_develop_ms", "load_and_fit_ms"]:
            aggregate[group][stage] = {key: median(r[stage][key] for r in selected)
                                       for key in ["before", "after", "reduction_pct"]}
        aggregate[group]["edits"] = {
            case: {key: median(r["edits"][case][key] for r in selected)
                   for key in ["before", "after", "reduction_pct"]}
            for case in selected[0]["edits"]
        }
    summary = {"exact_pixels_and_source_hashes_match": True, "photos": rows,
               "aggregate": aggregate,
               "aggregation": "Median across per-file medians; reduction_pct is the median paired per-file reduction, not the ratio of group medians."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2))
    for stage in ["load_ms", "cold_fit_ms", "full_develop_ms"]:
        print(stage, "median per-photo reduction:", round(median(r[stage]["reduction_pct"] for r in rows), 1), "%")
    print("All source, first preview, edit and full development hashes match.")


if __name__ == "__main__":
    main()
