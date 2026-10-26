#!/usr/bin/env python3
"""Summarize a parity run: classes overall and per input set, and every non-exact file group.

Usage: summarize.py <run-dir>   (reads <run-dir>/results.tsv, inputs.txt and header.txt)
"""
import collections
import os
import re
import sys

run = sys.argv[1]
rows = [l.rstrip("\n").split("\t") for l in open(os.path.join(run, "results.tsv"))][1:]
inputs = {}
for line in open(os.path.join(run, "keys.tsv")):
    key, path = line.rstrip("\n").split("\t")[:2]
    inputs[key] = path


def group(path):
    for name, pattern in [
        ("matrix", "/gen/matrix/"), ("jbigkit lowest layer", "/single/"),
        ("jbigkit layered", "/layers/"), ("libtiff test images", "/test/images/"),
        ("tiff-conformance", "tiff-conformance"), ("tests/libtiffpic", "libtiffpic"),
        ("tests/images", "tests/images"), ("tests/geotiff", "geotiff")]:
        if pattern in path:
            return name
    return "other"


def codec(name):
    m = re.search(r"__([a-z0-9-]+)__", name)
    return m.group(1) if m else "-"


print("# libtiff parity run\n")
print("".join("    " + l for l in open(os.path.join(run, "header.txt"))))
total = collections.Counter(r[2] for r in rows)
print(f"\n{len(rows)} directories: " + ", ".join(f"{k} {v}" for k, v in total.most_common()) + "\n")
print("| input set | " + " | ".join(sorted(total)) + " |")
print("|---|" + "---|" * len(total))
by_set = collections.defaultdict(collections.Counter)
for r in rows:
    by_set[group(inputs.get(r[0], ""))][r[2]] += 1
for name, c in sorted(by_set.items()):
    print(f"| {name} | " + " | ".join(str(c.get(k, 0)) for k in sorted(total)) + " |")
print("\n## Not exact\n")
groups = collections.defaultdict(list)
for r in rows:
    if r[2] == "exact":
        continue
    name = re.sub(r"^\d+_", "", r[0])
    # Numbered generator files (jbigkit sets) group by their pattern.
    base = name.split("__")[0] if "__" in name else re.sub(r"\d+", "N", name)
    detail = re.sub(r"\d+", "N", r[4])[:90]
    groups[(r[2], codec(name) if "__" in name else base, detail)].append(name)
for (cls, what, detail), names in sorted(groups.items()):
    print(f"- {cls}, {what}: {len(names)} ({detail})")
