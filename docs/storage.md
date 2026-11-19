# Storage: PostgreSQL and JSON files

A session keeps the company in memory, runs each command on a copy (so a failed command leaves no
trace), and hands the rows that command changed to a **store** in one transaction.

| Source | Store | Use |
| --- | --- | --- |
| `postgres://user:pass@host:5432/db?schema=acme` | PostgreSQL | Production, several users and processes |
| `path/to/company.erpcraft.json` | JSON file | Demos, tests, offline |

Every client resolves the source the same way (`erpcraft_engine::store::resolve_source`):
`--db <url>`, then `ERPCRAFT_DATABASE_URL`, then `--data <file>`, then `ERPCRAFT_DATA`, then
`./company.erpcraft.json`.

```powershell
$env:ERPCRAFT_DATABASE_URL = "postgres://postgres:secret@localhost:5432/erpcraft"
erpcraft-cli init --demo
erpcraft
```

## PostgreSQL layout

- **Database.** It is created on first connect if it doesn't exist.
- **One schema per company:** `?schema=` on the URL, else `ERPCRAFT_SCHEMA`, else `erpcraft`.
- **One table per BC table**, named as in BC (`"Sales Header"`, `"Cust. Ledger Entry"`). Columns are named as in BC (`"No."`, `"Sell-to Customer No."`) and typed:

  | AL type | PostgreSQL |
  | --- | --- |
  | Code, Text, Option | `text NOT NULL DEFAULT ''` |
  | Decimal | `numeric` |
  | Integer | `bigint` |
  | Boolean | `boolean` |
  | Date | `date`, NULL for a blank date |

  The primary key is the BC key. FlowFields are calculated, not stored.
- **`"$version"`** on every row, taken from the sequence `"$version_seq"` on each insert and update.
- **`"$deleted"`** records deletions, so other sessions drop those rows.
- **New fields** added in a later version are added with `ALTER TABLE … ADD COLUMN IF NOT EXISTS` on connect.

The data is plain SQL, so reports, BI tools and other services can read it directly:

```sql
SELECT "No.", "Sell-to Customer Name", "Posting Date"
FROM erpcraft."Sales Invoice Header"
ORDER BY "Posting Date" DESC;
```

## Several users at once

- **Refresh.** Before each command a session pulls the rows whose `"$version"` is above the last
  one it saw, and applies `"$deleted"`. The desktop app also refreshes every few seconds.
- **Optimistic concurrency.** An update or delete only succeeds if the row still has the version
  the session read. Otherwise nothing of that command is saved, and the user gets BC's message:
  *"Another user has modified the record for this Customer after you retrieved it from the
  database…"*. The session then refreshes, and the user can try again on current data.
- **Number series** are rows like any other. If two sessions take the same next number at once,
  the second save conflicts and is not written, so no number is ever used twice. Running that
  command again gives it the next number.

## Users and permissions

These follow BC's system tables:

- `User`
- `Permission Set`
- `Permission`: Object Type `Table Data` or `Command`, with an object name or `*`, and Read, Insert, Modify, Delete and Execute flags.
- `Access Control`: a user's permission sets.
- `User Personalization`: the user's saved work date.

How permissions are applied:
- While the User table is empty, everyone has SUPER, as in a new BC tenant.
- `company.init` creates the permission sets SUPER and VIEWER, and makes the user who runs it SUPER.
- A command needs Execute on its id (or `*`).
- The generic `record.*` commands also need Read, Insert, Modify or Delete on the table.

The session user is `ERPCRAFT_USER`, else the OS user name.

## Tests

`crates/engine/tests/postgres.rs` runs when `ERPCRAFT_TEST_DATABASE_URL` is set. Each test works
in its own schema and drops it afterwards.

```powershell
$env:ERPCRAFT_TEST_DATABASE_URL = "postgres://postgres:secret@localhost:5432/erpcraft"
cargo test -p erpcraft-engine --test postgres
```

## Limits today

- **TLS.** Connections don't use TLS yet (`NoTls`), so use PostgreSQL on localhost or a private network.
- **Memory.** A session holds the whole company in memory. That is fine for small and medium companies; paging large ledgers from SQL comes later.
- **Rare missed changes.** Refresh re-reads a window of 1,000 versions before the last one it saw. A change committed long after it took its version number could be missed until the next write to that row. Even then, saving never overwrites another session's work, because of the version check.
