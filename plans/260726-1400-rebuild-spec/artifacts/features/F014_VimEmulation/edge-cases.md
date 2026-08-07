# Edge Cases — F014_VimEmulation

| Scenario | What Happens | User-Facing Message |
|----------|--------------|---------------------|
| An ex command's range refers to a saved position (mark) that was never set | The command fails to resolve its range and does not run; the buffer is left unchanged | "mark {name} not set" |
| An ex command's range refers to a mark that exists but has no stored position | The command fails to resolve its range and does not run; the buffer is left unchanged | "mark {name} contains empty anchors" |
| Developer types an ex command that matches no known command or pattern | Nothing is executed against the buffer; the command line simply shows no matching suggestion | "None — no matching command found, command line stays open for correction" |
| Developer switches from one visual selection type to another very quickly (e.g. character-select then immediately line-select) | The selection is recomputed cleanly from the original starting point rather than stacking the two selections together | "None — silent handling, selection just updates" |
| Vim mode is turned off while the developer is mid-selection or mid-insert | Vim's mode-tracking stops applying to that editor; the editor falls back to standard (non-modal) text-editing behavior | "None — silent handling" |
| A saved position (global mark) points at a file that has since been moved, renamed, or deleted | The mark's last-known location is kept in storage, but it is not reattached to any open file until a matching file is found again | "None — the mark is simply unavailable until the file reappears" |
