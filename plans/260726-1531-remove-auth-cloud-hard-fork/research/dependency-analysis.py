#!/usr/bin/env python3
"""Reverse-dependency analysis for the Zed hard-fork deletion set."""
import json, sys
from collections import defaultdict

META = "/private/tmp/claude-501/-Users-tgiap-dev-devs-zode/e87881ac-1bb0-4e75-96fe-d86658df95c3/scratchpad/meta.json"
meta = json.load(open(META))

members = {}          # name -> pkg
for pkg in meta["packages"]:
    members[pkg["name"]] = pkg
names = set(members)

# name -> set(dep names)  split by kind
normal_deps = defaultdict(set)
dev_deps = defaultdict(set)
build_deps = defaultdict(set)
for name, pkg in members.items():
    for d in pkg["dependencies"]:
        dn = d["name"]
        if dn not in names:
            continue  # external crate
        kind = d.get("kind")  # None = normal, "dev", "build"
        if kind == "dev":
            dev_deps[name].add(dn)
        elif kind == "build":
            build_deps[name].add(dn)
        else:
            normal_deps[name].add(dn)

# reverse maps
rev_normal = defaultdict(set)
rev_dev = defaultdict(set)
for a, ds in normal_deps.items():
    for d in ds:
        rev_normal[d].add(a)
for a, ds in dev_deps.items():
    for d in ds:
        rev_dev[d].add(a)

# CORRECTIONS after verification:
# - cloud_api_types  REMOVED from seed: extension system needs ExtensionMetadata/ExtensionProvides
# - notifications    REMOVED from seed: status_toast is generic UI, used by 8 survivors -> gut, don't delete
# - copilot_ui       ADDED: only depends on copilot
SEED = set("""
collab collab_ui call channel livekit_client livekit_api copilot_ui
agent agent_ui agent_servers agent_settings ai_onboarding
language_models language_model language_model_core language_models_cloud
acp_thread acp_tools context_server
edit_prediction edit_prediction_ui edit_prediction_cli edit_prediction_types
edit_prediction_context edit_prediction_metrics
web_search web_search_providers
anthropic open_ai google_ai bedrock deepseek mistral ollama open_router
vercel_ai_gateway lmstudio codestral copilot copilot_chat open_ai_compatible
cloud_llm_client cloud_api_client
auto_update auto_update_ui auto_update_helper crashes
remote_server remote_connection
eval_cli eval_utils zeta
""".split())

print("=" * 78)
print("SEED entries that are NOT real workspace crates (typo / renamed):")
ghosts = sorted(SEED - names)
print("  " + (", ".join(ghosts) if ghosts else "(none)"))
SEED &= names
print(f"\nSEED size (real crates): {len(SEED)}  /  workspace total: {len(names)}")

# Iteratively grow: a crate joins if ALL its non-dev dependents are already in the set
# (i.e. it becomes orphaned once the seed is gone)
delete = set(SEED)
while True:
    added = set()
    for cand in names - delete:
        dependents = rev_normal.get(cand, set())
        if dependents and dependents <= delete:
            added.add(cand)
    if not added:
        break
    delete |= added

orphans = delete - SEED
print(f"\n{'='*78}")
print(f"ORPHANED by the seed deletion ({len(orphans)}) — nothing else uses them anymore:")
for c in sorted(orphans):
    print(f"  + {c:<34} (was used only by: {', '.join(sorted(rev_normal[c]))[:70]})")

print(f"\n{'='*78}")
print(f"TOTAL DELETE SET: {len(delete)} crates")

# Who OUTSIDE the delete set still depends on something INSIDE it -> must be fixed
print(f"\n{'='*78}")
print("SURVIVORS THAT MUST BE PATCHED (real deps into the delete set):")
survivors = names - delete
blast = {}
for s in sorted(survivors):
    hits = normal_deps[s] & delete
    if hits:
        blast[s] = hits
for s, hits in sorted(blast.items(), key=lambda kv: -len(kv[1])):
    print(f"  {s:<26} -> {', '.join(sorted(hits))}")
print(f"\n  => {len(blast)} crates need code changes.")

print(f"\n{'='*78}")
print("SURVIVORS with only DEV-dependencies into the delete set (tests only):")
devblast = {}
for s in sorted(survivors):
    hits = dev_deps[s] & delete
    if hits and s not in blast:
        devblast[s] = hits
for s, hits in sorted(devblast.items()):
    print(f"  {s:<26} -> {', '.join(sorted(hits))}")
print(f"\n  => {len(devblast)} crates need only test fixes.")

# Sanity: crates I explicitly want to KEEP
print(f"\n{'='*78}")
print("KEEP-LIST SANITY CHECK:")
for k in ["client", "telemetry", "telemetry_events", "remote_server", "extension_host",
          "proto", "rpc", "audio", "feature_flags", "settings_content", "workspace",
          "editor", "project", "title_bar", "command_palette", "zed", "denoise"]:
    if k not in names:
        print(f"  {k:<20} !! not a crate")
        continue
    status = "IN DELETE SET (!!)" if k in delete else "kept"
    into = normal_deps[k] & delete
    print(f"  {k:<20} {status:<20} deps into delete set: {', '.join(sorted(into)) if into else '-'}")
