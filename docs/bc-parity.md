# Parity with Business Central

ERPCraft's pages follow the BC W1 Base Application. `tools/bc-parity` keeps them in line with the AL sources in a local BCApps checkout (default `D:\Kerja\BCApps\src\Layers\W1\BaseApp`, or set `--root` / `BCAPPS_W1`).

## Measure

```powershell
cargo run -p bc-parity -- --summary          # one row per page
cargo run -p bc-parity -- --page customer-card
cargo run -p bc-parity -- --summary --check  # exit 1 while a page differs from BC
```

The tool reports these columns for every page that has a BC object id:

| Column | Meaning |
| --- | --- |
| Missing | BC shows the field, our table has it, our page doesn't. |
| Needs table field | BC shows the field and our table lacks it. These are mostly FlowFields and types ERPCraft doesn't have yet. |
| Extra | We show a field that BC hides or doesn't have. |
| Misplaced, Order, Tabs | The field sits in a different FastTab, the order differs, or the FastTabs differ. |
| Actions missing | BC actions we don't have, mostly functions not built yet. |

A field counts as shown unless it has `Visible = false`, or only application areas the Essential experience hides (Manufacturing, Planning, Warehouse, Service, ADCS, Advanced). `--check` fails on Missing, Extra, Misplaced, Order and Tabs.

## Generate

```powershell
cargo run -p bc-parity -- gen
```

This rewrites `crates/engine/src/schema/bc_gen.rs` from `schema::hand_written()`, so running it again gives the same file. Don't edit the generated file by hand. The generated layer adds the following on top of the hand-written schema:

- **Table fields.** Every field a BC page of ours shows, with BC's type, caption, tooltip, `TableRelation` to a table we have, and `Editable`. DateTime and Time fields are read-only text for now.
- **Captions.** Hand-written fields get BC's caption, plus its tooltip where they have none.
- **Card layouts.**
  - FastTabs and field order.
  - Promoted fields: their values show on a collapsed tab's header.
  - "Show more" fields (`Importance = Additional`).
  - Page caption overrides, such as Contact shown as "Contact Name".
  - Sub-group headings.
  - Column breaks at sub-groups.
- **List columns.** The columns in BC's order.
- **Navigation actions.** For example "Ledger Entries", when the target page exists here and its `RunPageLink` fields resolve.
- **Action bar categories.** BC's `area(Promoted)` groups. "Process" shows as Home, and everything else is under "More options".

Hand-written definitions win. To change something BC defines, change the hand-written schema or the generator, then run `gen` again.

## What the UI takes from BC

- **Card captions.** Each caption has a dotted leader. Clicking it opens a callout with the caption and the field's description.
- **Currency in captions.** "(LCY)" shows as the G/L Setup's Local Currency Symbol, for example "Balance (Rp)".
- **Card dialog.** It covers everything below the black app bar.
- **Wide lists.** They scroll sideways.
