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
| `read_only` | The connection refused a write — a read-only transaction, or an account without the privilege to write. **Not** `syntax` — the user is told the column is read-only, not that their SQL is wrong |
| `authentication` | Credentials rejected. The one error worth offering a password prompt for |
| `connection` | Could not reach the server, or lost it mid-call |
| `syntax` | The engine rejected the statement |
| `unknown_connection` | The call named a connection this driver does not have |
| `unsupported` | This driver does not implement the method |
| `cancelled` | Stopped at the caller's request — not a failure to report as one |
| `conflict` | An `update_rows` change matched a number of rows other than one, so the batch was rolled back. See [Updating rows](#updating-rows) |
| `invalid_value` | The engine rejected a value in an `update_rows` change, or the request asked for something the table cannot do (a view that is not updatable, a binary column, too many changes), so the batch was rolled back |
| `internal` | Anything else — including a lock that could not be had in time and a deadlock, which are not `conflict`: that code says *the row changed*, and a row someone else is holding says nothing of the kind |

An error may also carry `data`, naming where in an `update_rows` call the fault
was, when the driver can tell: `{ "change": 2, "column": "age" }`, both optional.
A client reads whatever part of `data` it understands and ignores the rest — a
field of an unexpected type costs it the hint, never the answer.

**A client must treat a `code` it does not know as a plain failure** — Zode
reads it as `other` and shows the `message`. Without that, one new code makes a
whole answer undecodable and the caller waits out its timeout for an error that
had already arrived. Codes are added without a version bump on the strength of
that rule, and only ever in answer to a method or capability the old side never
used: a driver emits `conflict` and `invalid_value` only for `update_rows`, which
a client older than it never sends.

## The nine methods

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
| `update_rows` | `{ connection_id, schema, table, changes }` | `{ updated }` |

`update_rows` is optional — see [Updating rows](#updating-rows).

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
- `row_updates` — whether `update_rows` is implemented. Absent or `false`
  means it is not, and Zode never sends one: the grid stays read-only.

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

**`query` runs exactly one statement.** A string that holds two must be
refused before either runs, not run whole: `SELECT 1; COMMIT; UPDATE …` ends
the read-only transaction the first statement opened and writes with the rest.
Wrapping the text in a script of the driver's own does not help — whatever
protocol takes several statements takes these. So the driver hands the text to
the engine in a form that takes one: PostgreSQL declares the cursor over it with
a prepared (extended-protocol) statement, which the server refuses if it holds
more than one; MySQL prepares the statement it is about to send, because a
prepare is parsed with multi-statement support off, and sends only what was
prepared; SQLite's prepare already stops at the first statement. The refusal
is a `syntax` error (or `read_only`), never a success.

`query` stays read-only on every connection. The one way to write is
`update_rows`, below, which carries structured values and no SQL text — so
there is nothing to inspect, and nothing that could be typed to turn it on.

## Updating rows

`update_rows` is how an edited cell reaches the database, and the only write a
driver offers. A driver that implements it reports `row_updates: true` at
`initialize`; one that does not answers `unsupported` and is never asked.

```json
→ {"jsonrpc":"2.0","id":9,"method":"update_rows","params":{
    "connection_id":"c1","schema":"public","table":"users",
    "changes":[{
      "key": [{"column":"id",   "value":{"kind":"text","value":"7"}}],
      "set": [{"column":"email","value":{"kind":"null"}}]}]}}
← {"jsonrpc":"2.0","id":9,"result":{"updated":1}}
```

A value is `{"kind":"null"}` or `{"kind":"text","value":"…"}`: what the person
typed, or what the key's cell displayed. Text is never an instruction to store
NULL — an empty string is a value, and NULL is its own kind.

Rules a driver must hold to:

1. **Bind, never concatenate.** Values go in as parameters and are cast to the
   column's real type by the engine. No value becomes part of a statement's text.
   The engine applies its own *assignment* rules to the text, so a value that
   does not fit the column (`varchar(5)`, `char(2)`, `bit(3)`, a domain over
   them) is refused. A driver must never name a type of its own for the cast: an
   explicit `CAST(x AS varchar(5))` truncates, and `CAST(x AS character)` is
   `char(1)`.
2. **Each change addresses one row by its whole key**, and must change exactly
   one. Any change that touches zero or several rows answers `conflict`, with a
   `detail` such as `row 3 matched 0 rows` and `data.change` set.
3. **One transaction.** Every change in the call is applied or none is. A
   `conflict`, an `invalid_value` or a lost connection leaves the table as it was.
4. **A value the engine rejects** — a failed cast, a constraint — answers
   `invalid_value` with the engine's words in `detail`, and `data.change` and
   `data.column` when the driver can tell which.
5. **An empty `changes` is `{ "updated": 0 }`** and opens no transaction.
6. **`updated` is the number of changes** on success.
7. **A connection opened read-only by policy answers `read_only`.** Nothing
   ships that way today; the rule is here so the first one that does cannot be
   made to write by this method.
8. An unknown `connection_id` answers `unknown_connection`, as everywhere.
9. **A binary column is never set.** The grid only knows the size of such a
   cell, so text typed into one is refused as `invalid_value`, "binary columns
   cannot be edited as text", with `data.column` set. SQLite, where any column
   may hold a blob, also looks at what the cell holds before overwriting it.
10. **At most 1000 changes per call** (`MAX_CHANGES_PER_CALL`). A larger call is
    refused as `invalid_value`, "too many changes in one save", before the
    driver sees it — `serve` does this for every driver. A client with more
    saves them in several calls.
11. **A write never waits for ever.** The connection is held while it waits, so
    browsing waits too. The SQL drivers bound the wait (PostgreSQL
    `lock_timeout` 5 s and `statement_timeout` 30 s for the transaction; MySQL
    `innodb_lock_wait_timeout` 5 s for the call; SQLite's busy timeout 5 s) and
    answer a lock that could not be had, or a deadlock, as `internal` with a
    message that says nothing was saved. Not `conflict`, not `connection`.
12. **A table that cannot roll back is not edited.** MySQL tables on MyISAM or
    MEMORY answer `invalid_value` before anything is written, because rule 3
    cannot be kept there.
13. **A refusal says what kind it was.** `invalid_value` for what the engine
    refused about the value or the target (a constraint, a failed cast, a view it
    cannot update through), `read_only` for a missing privilege, `internal` for a
    lock, a deadlock or a timeout, `conflict` only for the row-count rule.

**MongoDB.** `schema` is the database and `table` the collection. A document is
addressed by `_id` alone: the key's text is tried as an ObjectId (24 hex), then
an integer, then a string, and must name exactly one document. Each value is
converted to the type the field holds in that document; a field the document
lacks, or holds as null, takes the type most of the first 20 documents of the
collection give it, and a string where none of them holds it. Only top-level
fields are editable. Every key and value is checked before the first write, so
a missing document or an unconvertible value is refused with nothing changed on
any server. Each write is filtered on the type every edited field had when it
was read (`$type`, or `$exists: false` for a field the document lacked), so a
field another client changed in between makes the save a `conflict` and not a
value of the wrong type. The writes run in one transaction on a replica set or
sharded cluster (`hello`, or `isMaster` on a server older than 4.4.2, says
which). A standalone server has no transactions, so the changes are applied in
order; if one of those writes fails, the changes before it stay saved and the
error says how many. A refusal is sorted by the server's code: validation and
duplicate-key codes are `invalid_value`, `Unauthorized` is `read_only`,
`WriteConflict` is `conflict`, a primary changing hands is `connection`, and
anything else is `internal`.

Both directions of version skew are safe by construction. A Zode newer than the
driver sees no `row_updates` and offers no editing; a driver newer than Zode is
never sent the method, so its new error codes are never produced for a client
that cannot read them.

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
- `update_rows` is `unsupported` unless `row_updates` is claimed; a driver that
  claims it takes an empty batch as `{ "updated": 0 }` and refuses an unknown
  connection, and refuses a batch past `MAX_CHANGES_PER_CALL` as
  `invalid_value`. `row_update_suite` additionally checks, against a writable
  fixture, that an edit lands, that a batch with one bad change changes
  nothing (`conflict`), and that a rejected value is `invalid_value`.
- A null crosses as `{"kind":"null"}` and an empty string does not.
- `limit`/`offset` really page: the second page is not the first one again, and
  a full page sets `truncated`.
- A call naming a connection the driver does not have answers
  `unknown_connection` rather than reaching into whichever one it does have.

The shipped drivers all run it, SQLite included — that one needs no server, so
it is the run that keeps the harness itself honest.
