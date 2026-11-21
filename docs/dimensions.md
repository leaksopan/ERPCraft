# Dimensions

ERPCraft keeps dimensions the way Business Central does:

- one combination of values per Dimension Set ID;
- documents, journals and entries store only that ID;
- their Global and Shortcut Dimension 1 and 2 fields show the first two values.

## Tables and pages

| BC object | ERPCraft | Page |
| --- | --- | --- |
| 348 Dimension | `Dimension` | Dimensions (536) |
| 349 Dimension Value | `Dimension Value` | Dimension Values (537) |
| 352 Default Dimension | `Default Dimension` | Default Dimensions (540) |
| 480 Dimension Set Entry | `Dimension Set Entry` | Dimension Set Entries (479) |

General Ledger Setup names the dimensions in each slot:

- Global Dimension 1 and 2 Code;
- Shortcut Dimension 1–8 Code, where 1 and 2 follow the globals.

## How values flow

- **Master cards.** Setting Global Dimension 1/2 Code on a customer, vendor, item, G/L account, bank account or fixed asset card creates, changes or removes its Default Dimension. "Dimensions" (Alt+D) on these cards opens the Default Dimensions list for that record, where more dimensions can be added. New lines take the record from the list's filters.
- **Documents.**
  - Choosing the customer or vendor sets the header's dimension set from that party's default dimensions.
  - A line starts from the header's set, and the item's, G/L account's or fixed asset's defaults override it.
  - Typing a value in a Shortcut Dimension field changes the record's set.
  - Identical combinations share one set.
- **Posting.** G/L, customer, vendor and bank entries carry the document's or journal line's Dimension Set ID and Global Dimension 1/2 Code. The ledger entries' Shortcut Dimension 3–8 Code fields are FlowFields over the set, as in BC.
- **Viewing.** "Dimensions" (Alt+D) on documents, posted documents and entries shows their set.

## In the UI

- A dimension field is captioned with its dimension's Code Caption, as BC's CaptionClass does: "Department Code", not "Shortcut Dimension 1 Code".
- Its lookup lists that dimension's values.

The demo company has three dimensions:

- DEPARTMENT (ADM, SALES, SERVICE) as global dimension 1;
- AREA (JKT, BDG, SBY, YOG) as global dimension 2;
- PROJECT (FLEET, RETAIL) as shortcut dimension 3.

Customers default to SALES and their area, vendors to ADM, and items to SALES (SERVICE for service items).

## Not covered yet

- Value Posting rules (Code Mandatory, Same Code, No Code).
- Editing a document's whole set in one page (Edit Dimension Set Entries).
- Updating lines when a header's dimensions change.
- Per-line dimensions on the G/L entries of a posted document: entries take the header's set.
