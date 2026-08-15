# Zode database driver protocol

**Version 1.** Frozen 2026-08-15, after a second engine was written against it
without a line changing in the UI that consumes it.

A driver is a process. Zode starts it, speaks JSON-RPC 2.0 over its stdio, and
kills it when the connection closes. This document is the whole contract — a
driver that follows it works, whatever language it is written in.

## Framing

One JSON object per line on stdin, one per line on stdout.

**Nothing else may be written to stdout.** A single stray line breaks the
framing for every message after it. Log to stderr; Zode drains it into its own
log.

Requests may arrive while an earlier one is still running — `cancel` always
does. Answers may come back in any order; that is what `id` is for.

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
← {"jsonrpc":"2.0","id":1,"result":{"protocol_version":1,"driver_name":"SQLite","capabilities":{}}}
```

Errors carry a coarse `code`, a short `message`, and the engine's own words in
`detail`:

```
← {"jsonrpc":"2.0","id":7,"error":{"code":"read_only","message":"this connection is read-only","detail":"…"}}
```

| `code` | When |
|---|---|
| `read_only` | The connection refused a write. **Not** `syntax` — the user is told the column is read-only, not that their SQL is wrong |
| `authentication` | Credentials rejected. The one error worth offering a password prompt for |
| `connection` | Could not reach the server, or lost it mid-call |
| `syntax` | The engine rejected the statement |
| `unknown_connection` | The call named a connection this driver does not have |
| `unsupported` | This driver does not implement the method |
| `cancelled` | Stopped at the caller's request — not a failure to report as one |
| `internal` | Anything else |

## The eight methods

| Method | Params | Result |
|---|---|---|
| `initialize` | `{}` | `{ protocol_version, driver_name, capabilities }` |
| `connect` | `{ url, secret? }` | `{ connection_id, default_schema? }` |
| `disconnect` | `{ connection_id }` | `{}` |
| `list_schemas` | `{ connection_id }` | `{ schemas: [{ name, is_default }] }` |
| `list_tables` | `{ connection_id, schema }` | `{ tables: [{ name, kind }] }` |
| `describe_table` | `{ connection_id, schema, table }` | `{ columns: [{ name, type_name, nullable, primary_key }] }` |
| `query` | `{ connection_id, sql, limit, offset, request_id }` | `ResultSet` |
| `cancel` | `{ connection_id, request_id }` | `{}` |

`capabilities` — every field defaults to the cautious answer, so a driver
written against an older version of this struct stays correct:

- `multiple_schemas` — whether the engine has more than one schema worth
  showing. A driver that says `false` gets its tables drawn directly under the
  connection, saving a level nobody would ever collapse.
- `cancellation` — whether `cancel` does anything. A driver that cannot
  interrupt says so, and no button is offered that would lie.
- `identifier_quote` — the character this engine wraps identifiers in. Absent
  means the SQL standard's double quote, which SQLite and PostgreSQL accept.
  MySQL is why this exists: it wants backticks unless `ANSI_QUOTES` is set,
  which is not the default and is not a driver's to change on a session the
  user also types into. Zode quotes with whatever a driver reports, so an engine
  with different rules is a driver-side change and nothing more.
- `connection_form` — what to ask for when someone adds a connection. Absent
  means Zode asks for a URL and nothing else, which is right for a driver
  written before this existed and for any engine whose DSN does not decompose.

```json
{ "connection_form": {
    "fields": [
      { "key": "host", "label": "Host", "default": "localhost", "url_encoded": true },
      { "key": "password", "label": "Password", "secret": true }
    ],
    "url_template": "engine://{host}" } }
```

Every field is required except a `secret` one — a server that wants no password
is served by leaving it blank; anything else left blank is an address with a
hole in it. A driver that needs an optional non-secret field gives it a
`default` instead, so what the user sees is always a complete answer they can
edit.

Three rules follow from what these fields are for:

1. **A `secret` never enters the URL.** It goes to the OS keychain, keyed by the
   URL. The URL is written into a settings file people share and back up.
2. **`url_encoded` is per field and off by default.** A file path *is* the
   template, and encoding it turns every separator into `%2F`. A driver whose
   template is a real URL turns it on for the parts inside the authority, where
   an unescaped `@` in a user name would change where the host begins.
3. **The template is whole, not assembled.** Only the driver knows whether its
   engine wants a scheme, a socket or a bare path — a client that assembled DSNs
   would be a client that knows engines.

`kind` is `table`, `view` or `materialized_view`. The last is separate because
reading one costs nothing while reading a view can cost a great deal.

## Values

```json
{ "kind": "null" }
{ "kind": "text",      "value": "a@b.com" }
{ "kind": "number",    "value": "123456789012345678901234567890.0000000001" }
{ "kind": "bool",      "value": true }
{ "kind": "binary",    "byte_len": 4 }
{ "kind": "json",      "value": "{\"a\":1}" }
{ "kind": "timestamp", "value": "2026-08-15 10:00:00" }
```

Four rules, and they are the whole reason this shape is what it is:

1. **The driver formats.** It is the only layer that knows what its engine's
   types mean. The `kind` says how to *present* the value, not what the engine
   called it — so a grid right-aligns numbers without a table of per-engine type
   names.
2. **`null` is a kind, not an absent field.** A null and an empty string are
   different answers, and that matters in a database client more than almost
   anywhere else.
3. **Numbers cross as text.** `numeric(38,10)` and `u64` both lose digits
   through a float, and a client that quietly rounds what it is asked to display
   is worse than useless.
4. **Binary crosses as a size.** A large blob through a line-delimited JSON pipe
   is how a driver stalls the editor reading it.

Anything a driver has no kind for — arrays, enums, ranges, composites,
extension types — is `text` carrying the engine's own rendering. That is the
honest answer; a kind per type would be a second type system nobody can keep
current.

## Paging

`limit` and `offset` are always set. **A page is what crosses the wire, never a
whole table.** Ask the engine for one row more than `limit`, drop it, and set
`truncated` — that answers "is there more" without a `COUNT(*)`.

```json
{ "columns": [{ "name": "id", "type_name": "integer" }],
  "rows": [[{ "kind": "number", "value": "1" }]],
  "truncated": true,
  "elapsed_ms": 3 }
```

Page in the server. The shipped drivers show three ways, and the third is a
warning:

- **SQLite** steps its statement and skips.
- **PostgreSQL** uses `MOVE`/`FETCH` on a `NO SCROLL` cursor.
- **MySQL** has no cursor a client can `FETCH` from, so it wraps the statement in
  a derived table with `LIMIT`/`OFFSET`. That fails for `EXPLAIN`, `SHOW`,
  `DESCRIBE` and CTEs, which it runs whole and truncates in the driver.

The first two page any statement that produces rows, including the ones someone
reaches for when a query behaves strangely. A driver that cannot do that should
say which statements it cannot page rather than quietly returning everything.

## Read-only

**Read-only is the driver's job, and it must be the engine that refuses.** Zode
never inspects a statement to guess whether it writes.

It also must not be undoable from the SQL the user types, which rules out the
obvious approach on every engine tried so far:

| Engine | What does **not** work | What does |
|---|---|---|
| SQLite | `PRAGMA query_only = ON` — the user can type `OFF` | Open the file with `SQLITE_OPEN_READ_ONLY` |
| PostgreSQL | `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` — the user can type `READ WRITE` | Run every statement inside its own `BEGIN TRANSACTION READ ONLY` |
| MySQL | `SET SESSION TRANSACTION READ ONLY` — the user can type `READ WRITE` | Run every statement inside its own `START TRANSACTION READ ONLY` |

A read-only mode the user can switch off by typing is not one.

## Cancelling

`cancel` names a `request_id` the **caller** chose, because a query worth
cancelling is usually one that has not answered anything yet.

Two rules:

- Check the id against the query that is *actually running* before interrupting.
  A `cancel` arriving after its query finished must not reach through and kill
  the next one.
- `cancel` arrives while `query` is still running, so whatever a driver needs in
  order to interrupt must be reachable without the lock the query holds.

## Version policy

`initialize` reports `protocol_version`. Zode refuses a driver whose number
differs from its own, and says both numbers — otherwise nobody can tell which
end to fix.

Version 1 is frozen. A breaking change means version 2, and every driver pinned
to 1 stops working; that cost is the point. Adding an optional field with a
cautious default is not breaking and does not need a new number.

## Conformance

`database::driver_test_suite` starts a driver binary, speaks this document to
it, and checks the answers. It is Rust, so a driver written in something else
cannot run it directly — but what it asserts is the shortest list of things a
driver must get right, and it is worth reading as one:

- `initialize` reports this version, and a name.
- `connect` names the connection; `list_schemas` returns at least the one it is
  pointed at.
- A write comes back as `read_only` — **not** `syntax`, which is the mistake
  that tells the user to check SQL they wrote correctly.
- A null crosses as `{"kind":"null"}` and an empty string does not.
- `limit`/`offset` really page: the second page is not the first one again, and
  a full page sets `truncated`.
- A call naming a connection the driver does not have answers
  `unknown_connection` rather than reaching into whichever one it does have.

The shipped drivers all run it, SQLite included — that one needs no server, so
it is the run that keeps the harness itself honest.
