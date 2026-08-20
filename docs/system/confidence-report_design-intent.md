---
source_artifact: docs/system/design-intent.md
claims_total: 11
claims_with_evidence: 0
confidence_derived: 0.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/system/design-intent.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                 | Section               | Evidence (file:line) | Status ○/△ |
| ------------------------------------------------------------------------------------- | --------------------- | -------------------- | ---------- |
| why"), (c) `docs/system/architecture.md` / `business-rules.md`, or (d) `` tags.       | Preamble              | —                    | △          |
| or doc found. — no commit message or doc states the business motive; inferred from    | Architecture Choices  | —                    | △          |
| gap." — no ADR/comment resolves this; treated here as an open question, not settled   | Architecture Choices  | —                    | △          |
| — no single comment states this as a project-wide rule, but the business-rules.md     | Patterns & Trade-offs | —                    | △          |
| The business motive for stripping collab/auth/AI/cloud (turning a multiplayer,        | [INFERRED] Appendix   | —                    | △          |
| Whether the extension capability model's lack of a runtime grant/revoke UI is         | [INFERRED] Appendix   | —                    | △          |
| Fire-and-forget background-task handling as intentional project-wide house style      | [INFERRED] Appendix   | —                    | △          |
| anywhere found — see the tag above. If a non-technical driver exists (e.g. licensing, | Open Questions        | —                    | △          |
| ``.                                                                                   | Handoff Summary       | —                    | △          |
| ** ratio**: 3 of 13 total substantive claims (§ Architecture Choices: 6, § Patterns   | Handoff Summary       | —                    | △          |
| are explicitly `` — roughly 23%. Every other claim carries a direct                   | Handoff Summary       | —                    | △          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

- Preamble: why"), (c) `docs/system/architecture.md` / `business-rules.md`, or (d) `` tags.
- Architecture Choices: or doc found. — no commit message or doc states the business motive; inferred from
- Architecture Choices: gap." — no ADR/comment resolves this; treated here as an open question, not settled
- Patterns & Trade-offs: — no single comment states this as a project-wide rule, but the business-rules.md
- [INFERRED] Appendix: The business motive for stripping collab/auth/AI/cloud (turning a multiplayer,
- [INFERRED] Appendix: Whether the extension capability model's lack of a runtime grant/revoke UI is
- [INFERRED] Appendix: Fire-and-forget background-task handling as intentional project-wide house style
- Open Questions: anywhere found — see the tag above. If a non-technical driver exists (e.g. licensing,
- Handoff Summary: ``.
- Handoff Summary: ** ratio**: 3 of 13 total substantive claims (§ Architecture Choices: 6, § Patterns
- Handoff Summary: are explicitly `` — roughly 23%. Every other claim carries a direct

## Risk Flags

- Low citation coverage (0%) -- most claims are marker-tagged, not cited.
