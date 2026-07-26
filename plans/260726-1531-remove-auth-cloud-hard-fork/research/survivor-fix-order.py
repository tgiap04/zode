#!/usr/bin/env python3
"""Derive the reverse-topological fix order for the 17 survivors.

Phase 1 step 4. Red team finding: derive this programmatically from cargo
metadata, never by eyeballing the dependency graph SVG.

Errors flow downstream only (dependency -> dependent), so fixing in
reverse-topological order means each crate is touched exactly once.
"""
import json
import subprocess
import sys
from collections import defaultdict

meta = json.loads(
    subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, check=True,
    ).stdout
)
members = {p["name"]: p for p in meta["packages"]}
names = set(members)

# internal (workspace) dependency edges, non-dev only
deps = defaultdict(set)
for name, pkg in members.items():
    for d in pkg["dependencies"]:
        if d["name"] in names and d.get("kind") != "dev":
            deps[name].add(d["name"])

# The 17 survivors needing code changes.
# 15 from the crate-level dependency graph, plus the two the graph could not
# see because they depend on KEPT crates (client, cloud_api_types) at the
# symbol level: onboarding (red team finding 1) and project (finding 5).
SURVIVORS = [
    "activity_indicator", "client", "cloud_api_types", "diagnostics",
    "edit_prediction_types", "file_finder", "git_ui", "language_tools",
    "notifications", "onboarding", "project", "remote_connection",
    "remote_server", "settings_content", "settings_ui", "title_bar",
    "workspace", "zed",
]
missing = [c for c in SURVIVORS if c not in names]
if missing:
    sys.exit(f"not real crates: {missing}")

survivor_set = set(SURVIVORS)

# Kahn over the subgraph induced by the survivors: a survivor is ready when
# every survivor it depends on has already been emitted.
indeg = {c: len(deps[c] & survivor_set) for c in SURVIVORS}
emitted, order = set(), []
while len(order) < len(SURVIVORS):
    ready = sorted(c for c in SURVIVORS
                   if c not in emitted and not (deps[c] & survivor_set - emitted))
    if not ready:
        cycle = sorted(set(SURVIVORS) - emitted)
        sys.exit(f"dependency cycle among survivors: {cycle}")
    for c in ready:
        order.append(c)
        emitted.add(c)

print("# Survivor fix order (reverse-topological, leaves first)")
print("#")
print("# Derived by research/survivor-fix-order.py from cargo metadata.")
print("# Fix in THIS order and each crate is touched exactly once — errors")
print("# only flow dependency -> dependent.")
print("#")
print("# Use `cargo check -p <crate>`, never `--workspace`, until Phase 11.")
print()
for i, c in enumerate(order, 1):
    within = sorted(deps[c] & survivor_set)
    note = f"  # after: {', '.join(within)}" if within else "  # no survivor deps — a leaf"
    print(f"{i:2}. {c}{note}")

print()
print("# Phase assignment")
light = {"activity_indicator", "diagnostics", "language_tools", "notifications",
         "project", "workspace", "file_finder"}
heavy = {"settings_ui", "onboarding", "title_bar", "git_ui", "zed"}
p3 = {"edit_prediction_types", "settings_content", "cloud_api_types",
      "remote_connection", "remote_server"}
for c in order:
    ph = ("3" if c in p3 else "5" if c == "client"
          else "7" if c in light else "8" if c in heavy else "?")
    print(f"#   {c:<22} Phase {ph}")
