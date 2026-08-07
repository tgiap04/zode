# Edge Cases — F016_Search

| Scenario | What Happens | User-Facing Message |
|----------|--------------|---------------------|
| File finder query matches more than 100 files | Only the top 100 ranked matches are computed and shown — the rest are silently not returned | "None — list is simply capped at 100 rows" |
| File finder query matches no existing file | A "create new file" entry appears at the bottom of the list, offering to create that exact path | "(create new file at this path)" |
| A recent-history entry's file no longer exists on disk | The stale entry is dropped from the candidate list in the background before it can ever be shown | "None — silently filtered out" |
| Project search query is an invalid regex (regex mode on) | The query input is flagged with an inline error and no search runs until the query becomes valid | "{regex compiler error text}" |
| Project search matches more than 5,000 files or 10,000 total matches | The search stops accumulating further results; the shown count is suffixed with "+" to signal it's a floor, not an exact total | "{count}+ matches" |
| Buffer search query is left empty | Any existing highlighted matches are cleared and no search is performed | "None — highlights simply clear" |
| Buffer search query is an invalid regex (regex mode on) | The bar shows an inline error and clears stale highlights rather than crashing or showing wrong matches | "{regex compiler error text}" |
| Command-palette invocation log write fails (SQLite error) | The error is logged internally; the command the developer confirmed still runs normally | "None — silent, logged only" |
