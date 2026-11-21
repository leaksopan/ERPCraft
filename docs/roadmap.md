# Roadmap to Business Central parity

**Goal.** Every page, component and function of ERPCraft matches Business Central, as the user put it: "every single detail matters". BCApps W1 (`D:\Kerja\BCApps\src\Layers\W1\BaseApp`) is the source of truth for structure and logic. The live web client (CRONUS) is the reference for how things look and behave. Where BC calls Microsoft services (Dataverse, OneDrive, Copilot, Power Automate), ERPCraft may do something different (see [Out of scope](#out-of-scope)).

This file is the single to-do list. Update the numbers and tick the boxes at every checkpoint.

## What "100%" means

Each target is something a script or a screenshot can confirm, not a judgement call.

| # | Target | How to check | Now (2026-11-21) |
| --- | --- | --- | --- |
| 1 | Page structure matches BC: FastTabs, field order, columns | `cargo run -p bc-parity -- --summary --check` exits 0 | ✅ blocking 0 |
| 2 | Every field BC shows on our pages exists | "needs table field" in the summary = 0 | 67 |
| 3 | Every BC action on our pages exists and works | "actions missing" = 0, minus the agreed out-of-scope list | 1,323 |
| 4 | Every page looks and behaves like BC | the [visual checklist](#visual-parity-checklist) fully ticked | 7 / 111 pages |
| 5 | Every function has tests | `cargo test --workspace`; each new command has an integration test | ✅ 27 suites green |

Pages with a BC object id: 111. The baseline when the parity tool was built was 86 fields missing from pages, 1,037 missing table fields, 190 extra fields and 1,477 missing actions.

## How every checkpoint is done

1. **Pick.** Take the next unticked item from the current phase below.
2. **Read BC first.** Read the AL source: the table, the page, the codeunit behind the action. Open the same page in the live client (built-in browser) and click through it.
3. **Build it.**
   - Structure goes in the hand-written schema or the generator, never in `bc_gen.rs` by hand.
   - Logic is an engine command with a test (AGENTS.md).
4. **Verify.**
   - `cargo test --workspace`, with the PostgreSQL env.
   - `bc-parity --summary --check`.
   - A screenshot of the page in the app: `erpcraft --control 7980` and `erpcraft-cli --connect 7980 ui …`, built into `target-ui` so the user's own window stays open.
5. **Record.** Append a row to `.audit/bc-parity.tsv`, tick the box here, update the numbers above.
6. **Ship.** Commit with a message file and push to `main`. No asking ([memory: autonomous commit & push]).

## Done

- **Phases 1–4.** PostgreSQL store; users and permissions; journals and applications; ship/invoice posting; inventory and locations; fixed assets; PDF printing; list filter pane, sort and columns; BC API connector; wasm web build.
- **Parity tooling.** `tools/bc-parity` (measure and generate) and the generated layer `bc_gen.rs`:
  - fields, captions, tooltips, decimals (AutoFormatType);
  - FastTabs, Show more, sub-headings, column breaks;
  - list columns, navigation actions, action categories;
  - FlowFields from CalcFormula, with drill-downs.
- **Card.** Dotted caption leaders; caption callout; LCY symbol in captions; Show more in the tab header; covers everything below the app bar; BC input boxes with the icon inside; read-only toggles; status style.
- **List grid.** 34 px rows with row lines; header menu (sort, filter, What's this?); ⋮ row menu; current cell; arrow keys; drill-down links; resizable columns; back button on stacked lists.
- **Documents.** Lines part as BC's editable subform; Post and New, Preview Posting, Test Report, Post and Print, Copy Document, Select items; Print Confirmation, Work Order and Pro Forma; posted Statistics.
- **Items.** Copy Item, Adjust Inventory.
- **Subsystems.** Dimensions; Comments; Find entries (Navigate); Approvals; Attachments.

## Phase A: existing functions on every page BC has them

This is cheap and moves the action count fastest. The function exists but isn't offered everywhere BC offers it.

- [ ] Approvals, Send/Cancel Approval Request on **list** pages (sales/purchase quotes, orders, invoices, credit memos), acting on the selected row
- [ ] Co&mments on list pages and on journals (Gen. Journal Line comments)
- [ ] Statistics on list pages (customer, vendor, item lists; document lists)
- [ ] Post and Print, Test Report, Preview Posting on **journals** (general, cash receipt, payment, item, FA journals) and Post Batch
- [ ] Attach as PDF / Attachments on posted document **lists**
- [ ] Customer Statistics / Vendor Statistics actions on documents (BC opens the party's statistics)
- [ ] Dimensions-Multiple and Set Dimension Filter on lists

Done when "actions missing" drops for each of these captions to the pages BC really shows them on (check with `bc-parity`, grouped by caption).

## Phase B: document lifecycle

- [ ] **Incoming Documents** (Incoming Document, Incoming Document Attachment): Create from File, Select, View, Remove on documents and journals
- [ ] **Archive Document** and "No. of Archived Versions" (Sales/Purchase Header Archive)
- [ ] **Cancel / Correct posted invoices**: Corrective Credit Memo, Cancel, and the "Cancelled" / "Corrective" fields (Cancelled Document table)
- [ ] **Calculate Invoice Discount** (Cust./Vendor Invoice Disc.)
- [ ] Get Recurring Sales/Purchase Lines (Standard Codes)
- [ ] Move Negative Lines, Show All Lines / Show Lines with Issues
- [ ] **Prepayments**: prepayment %, Post Prepayment Invoice / Credit Memo, Pending Prepayment status
- [ ] Update Document (posted document header edits BC allows)
- [ ] "Paid", "Completely Shipped" and "Amt. Ship. Not Inv." fields

## Phase C: inventory depth

- [ ] **Item Tracking** (Lot / Serial No.): Item Tracking Lines, Item Tracking Entries, Document Line Tracking
- [ ] **Reservations**: Reserved Quantity, Reserve, Reservation Entries
- [ ] **Item Charges**: Qty. to Assign, Item Charge Assignment, Qty. Assigned
- [ ] Stockkeeping Units (Create Stockkeeping Unit)
- [ ] Assembly BOM / BOM Level, "Assembly BOM" field
- [ ] Item Availability by Period / Location / Event / BOM Level pages
- [ ] Item Substitutions ("Substitutes Exist"), Item References, Units of Measure per item, Item Variants

## Phase D: prices and discounts

- [ ] Price lists (BC's new pricing): Sales Prices, Purchase Prices, line discounts, Price and Discount Overview
- [ ] Prices used on document lines (best price), Customer Price Group / Disc. Group

## Phase E: journals

- [ ] Journal Batch management on journal pages (batch picker, Journal Batch action)
- [ ] Recurring journals, Post Batch from batch lists
- [ ] Deferral templates and Deferral Schedule

## Phase F: communication and contacts

- [ ] Contacts (CRM Contact, Contact Business Relation): the Contact action on customer and vendor cards
- [ ] Send / Send by Email / Sent Emails (Email Account, Document Sending Profile); SMTP is fine as the transport
- [ ] Report Selections per document

## Phase G: UI detail backlog

- [ ] Option captions that the page overrides (BC shows "Default (No)" where we show "Default")
- [ ] A native file picker for Attach a file (BC opens a file dialog; we type a path)
- [ ] "Resize" in the column header menu
- [ ] Role center cues such as Requests to Approve and Incoming Documents, and their actions
- [ ] Tell Me results grouped the way BC groups them
- [ ] Re-check the wasm web build after each phase

## Visual parity checklist

Each page is compared side by side with the live client: layout, components, hover and click behaviour.

- [x] Customer Card, Customer List
- [x] Item List, Item Card
- [x] Sales Order (header, lines part, totals)
- [x] Purchase Order
- [x] Posted Sales Invoice
- [ ] Vendor Card, Vendor List
- [ ] Sales Quote, Sales Invoice, Sales Credit Memo and their lists
- [ ] Purchase Quote, Invoice, Credit Memo and their lists
- [ ] Posted documents (credit memos, shipments, receipts, purchase side)
- [ ] Ledger entry lists (G/L, customer, vendor, item, bank, FA)
- [ ] Journals (general, cash receipt, payment, item, reclass, phys. inventory, FA)
- [ ] Chart of Accounts, G/L Account Card
- [ ] Bank Account Card and List
- [ ] Fixed Asset Card and List, FA setup pages
- [ ] Setup pages (General Ledger, Sales & Receivables, Purchases & Payables, Inventory, posting setups)
- [ ] Role center (headline, cues, action bar, navigation)
- [ ] Dimensions, Comments, Approvals and Attachments pages

## Out of scope

These BC features talk to Microsoft services. ERPCraft offers its own path or leaves them out. Each needs the user's decision before it's built.

- Dataverse coupling ("Coupled to Dataverse", Delete Coupling, synchronize)
- Power Automate ("Create approval flow"); ERPCraft's approvals are built in
- Copilot summaries and OneDrive file actions
- Edit in Excel. A plain .xlsx export could replace it.
- Job Queue (Remove From Job Queue). Posting runs at once.

## Known differences

- G/L entries of a posted document carry the header's dimension set, not each line's.
- Default dimension Value Posting rules are not enforced yet.
- DateTime fields are read-only text.
