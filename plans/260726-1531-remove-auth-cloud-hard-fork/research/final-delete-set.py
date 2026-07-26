#!/usr/bin/env python3
"""FINAL delete set after scout corrections + user decisions."""
import json
from collections import defaultdict

meta = json.load(open("/private/tmp/claude-501/-Users-tgiap-dev-devs-zode/e87881ac-1bb0-4e75-96fe-d86658df95c3/scratchpad/meta.json"))
members = {p["name"]: p for p in meta["packages"]}
names = set(members)

normal, dev = defaultdict(set), defaultdict(set)
for n, p in members.items():
    for d in p["dependencies"]:
        if d["name"] not in names:
            continue
        (dev if d.get("kind") == "dev" else normal)[n].add(d["name"])

rev = defaultdict(set)
for a, ds in normal.items():
    for d in ds:
        rev[d].add(a)

# FINAL SEED — after 4 scout corrections + 2 user decisions
SEED = set("""
collab collab_ui call channel livekit_client livekit_api
copilot copilot_chat copilot_ui
agent agent_ui agent_servers agent_settings ai_onboarding
acp_thread acp_tools
language_models language_model language_model_core language_models_cloud
edit_prediction edit_prediction_ui edit_prediction_cli
edit_prediction_context edit_prediction_metrics
web_search web_search_providers
anthropic open_ai google_ai bedrock deepseek mistral ollama open_router
lmstudio codestral
cloud_llm_client cloud_api_client
auto_update auto_update_ui auto_update_helper
crashes
eval_cli eval_utils
sidebar
""".split())
# EXPLICITLY KEPT (scout-corrected / user decision):
KEEP_INTENT = {
    "edit_prediction_types": "scout: only tie = client::EditPredictionUsage (3 lines); deleting = ~2500 lines in editor.rs",
    "remote_connection":     "user: keep SSH remote dev; patch 2 AutoUpdater call sites",
    "remote_server":         "user: keep SSH remote dev; patch out crashes:: (4 sites)",
    "remote":                "abstraction needed by project/workspace/extension_host",
    "context_server":        "scout: zero cloud deps; deleting = ~1900 lines in project",
    "cloud_api_types":       "extension registry DTOs (ExtensionMetadata/ExtensionProvides)",
    "notifications":         "status_toast used by 13 survivors; gut notification_store only",
    "client":                "gut auth, keep proto/rpc/Collaborator/ParticipantIndex",
    "telemetry":             "no-op send_event; 29 dependents",
    "recent_projects":       "survives with remote_connection kept",
    "editor":                "survives with edit_prediction_types kept",
    "project":               "survives with context_server kept",
}
ghosts = sorted(SEED - names)
SEED &= names

delete = set(SEED)
while True:
    add = {c for c in names - delete
           if rev.get(c) and rev[c] <= delete and c not in KEEP_INTENT}
    if not add:
        break
    delete |= add

print(f"ghosts (not real crates): {ghosts or 'none'}")
print(f"\nSEED: {len(SEED)}   ORPHANED: {len(delete - SEED)}   TOTAL DELETE: {len(delete)}")
print(f"\nOrphans pulled in automatically:")
for c in sorted(delete - SEED):
    print(f"  + {c}")

survivors = names - delete
blast = {s: normal[s] & delete for s in sorted(survivors) if normal[s] & delete}
devonly = {s: dev[s] & delete for s in sorted(survivors)
           if (dev[s] & delete) and s not in blast}

print(f"\n{'='*70}\nSURVIVORS NEEDING CODE CHANGES: {len(blast)}")
for s, h in sorted(blast.items(), key=lambda kv: -len(kv[1])):
    print(f"  {s:<22} ({len(h):>2}) {', '.join(sorted(h))}")
print(f"\nDEV-DEP ONLY (tests): {len(devonly)}")
for s, h in devonly.items():
    print(f"  {s:<22} {', '.join(sorted(h))}")

print(f"\n{'='*70}\nKEEP-LIST verification:")
for k, why in KEEP_INTENT.items():
    if k not in names:
        print(f"  !! {k} not a crate"); continue
    bad = "IN DELETE SET (BUG!)" if k in delete else "kept ok"
    into = sorted(normal[k] & delete)
    print(f"  {k:<22} {bad:<20} must-patch: {', '.join(into) if into else '(none)'}")

print(f"\nWorkspace: {len(names)} -> {len(survivors)} crates ({len(delete)} removed, "
      f"{100*len(delete)//len(names)}%)")
