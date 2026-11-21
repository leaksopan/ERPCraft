# Roadmap to Business Central parity: everything

**Goal.** ERPCraft has every page, component and function of Business Central:

- the whole W1 Base Application (2,638 pages, 659 reports);
- the W1 apps Microsoft ships beside it (Shopify, E-Documents, Subscription Billing, Sustainability and about 70 more).

BCApps W1 (`D:\Kerja\BCApps\src`) is the source of truth for structure and logic. The live web client (CRONUS) is the reference for look and behaviour. The user put it as "every single detail matters".

**Two lists, one plan.**

- [`docs/bc-inventory.md`](bc-inventory.md) is the complete to-do list: every BC page and report, per area, ticked when ERPCraft has it. It is generated, never hand-edited:

  ```powershell
  cargo run -p bc-parity -- inventory
  ```
- This file orders that inventory into phases and keeps the targets and the working routine.

## What "100%" means

Each target is checked by a script or a screenshot, not by eye.

| # | Target | How to check | Now (2026-11-21) |
| --- | --- | --- | --- |
| 1 | Every Base Application page exists | inventory: pages ticked | 110 / 2,638 |
| 2 | Every Base Application report exists | inventory: reports ticked | 16 / 659 |
| 3 | Every W1 app's pages and reports exist (or are decided out) | inventory, W1 apps section | 0 / ~1,700 pages |
| 4 | Every page we have matches BC's structure | `cargo run -p bc-parity -- --summary --check` exits 0 | ✅ blocking 0 |
| 5 | Every field BC shows on our pages exists | "needs table field" = 0 | 67 |
| 6 | Every action BC offers on our pages exists and works | "actions missing" = 0 | 1,521 |
| 7 | Every page looks and behaves like BC | [visual checklist](#visual-parity-checklist) | 7 pages |
| 8 | Every function has tests | `cargo test --workspace` | ✅ 27 suites |

Targets 4–6 apply to the pages we already have. As new pages land they join the check, so those numbers can rise before they fall.

## The routine for every checkpoint

1. **Pick.** Take the next unticked item of the current phase, then its pages in [bc-inventory.md](bc-inventory.md).
2. **Read BC.** Read the AL source: tables, pages, the codeunits behind the actions. Then open the page in the live client (built-in browser) and click through it.
3. **Build.**
   - Tables and pages go in `crates/engine/src/schema/` with BC's object ids.
   - Layout comes from the generator (`bc-parity gen`), never from hand edits to `bc_gen.rs`.
   - Logic is an engine command with an integration test (AGENTS.md).
4. **Verify.**
   - `cargo test --workspace`, with the PostgreSQL env.
   - `bc-parity --summary --check`.
   - Screenshots in the app on port 7980, built into `target-ui`, so the user's window on 7979 stays open.
5. **Record.**
   - Run `bc-parity inventory`.
   - Tick here and update the numbers above.
   - Add a row to `.audit/bc-parity.tsv`.
6. **Ship.** Commit and push to `main`.

## Done so far

- **Phases 1–4.** PostgreSQL store; users and permissions; journals and applications; ship/invoice posting; locations and item journals; fixed assets; PDF printing; list filter, sort and columns; BC API connector; wasm web build.
- **Parity tooling.** `tools/bc-parity`: measure, generate, inventory.
- **Card and list.** Card components and the list grid match BC. Details are in [bc-parity.md](bc-parity.md).
- **Documents.**
  - Post and New, Post and Print, Preview Posting, Test Report;
  - Copy Document, Select items;
  - Print Confirmation, Work Order, Pro Forma;
  - posted Statistics.
- **Items.** Copy Item, Adjust Inventory.
- **Subsystems.** Dimensions, Comments, Find entries (Navigate), Approvals, Attachments.

## Phases

The order follows impact for Auto Craft (a workshop and auto-parts business): money and stock first, then the areas the business grows into, then the rest. Every area of BC appears exactly once.

### Phase A: what exists, everywhere BC offers it

- [x] Approvals, Send/Cancel Approval Request on document **lists**, acting on the selected row
- [x] Co&mments on lists and journals (journal approval Comments; line Comments and Dimensions from the Lines part's Line menu)
- [x] Statistics, Customer Statistics, Vendor Statistics on lists and documents (also Entry Statistics, Statistics by Currencies; items: Item Statistics, Entry Statistics, Turnover, Items by Location. Advanced Statistics matrix later)
- [x] Post and Print, Test Report, Preview Posting, Post Batch on **journals** (Preview Posting, Test Report, Renumber Document Numbers, Post and Print, Card, Ledger Entries done; Post Batch on order, invoice and credit memo lists done)
- [x] Card actions mirrored on their list pages (generator `mirror` pass)
- [x] Attachments and Attach as PDF on posted document lists
- [x] Dimensions-Multiple and Set Dimension Filter on lists (lists mark several rows: Ctrl / Shift + click, Select More; actions get them as `keys`)

### Phase B: Finance (Finance 344 pages / 86 reports, Bank 78 / 17)

- [x] **Financial reports** (account schedules): row and column definitions, Balance Sheet / Income Statement as BC builds them (budgets, analysis views, dimension filters, Excel export to do)
- [ ] G/L budgets, analysis views, accounting periods, Close Income Statement and year end (accounting periods, Create / Close Year, Close Income Statement, G/L budgets and budget columns done; analysis views, budget matrix to do)
- [ ] **Currencies**: exchange rates, currency on documents and journals, adjust exchange rates, realized and unrealized gains (done for customers and vendors; bank accounts in a currency, additional reporting currency, re-pricing lines when the currency changes to do)
- [ ] VAT: statements, VAT returns, VAT entries, VAT rate change, VAT registration numbers (VAT entries, statements with preview, Calc. and Post VAT Settlement done; VAT returns, rate change, unrealized VAT, reversing VAT entries to do)
- [ ] **Recurring journals**, allocations, allocation accounts, deferrals with Deferral Schedule (recurring general journal with F / V / B and reversing methods and allocations done; allocation accounts, deferrals to do)
- [ ] **Bank account reconciliation**, payment reconciliation journal, bank statement import, check printing (bank account reconciliation with Suggest Lines, Match Automatically, Post and bank account statements done; payment reconciliation journal, statement import, checks to do)
- [x] **Reminders and finance charge memos**, customer statements (reminder terms and levels, Create / Suggest / Issue, finance charge terms with interest, Issue, Reminder/Fin. Charge Entries, Statement with aging)
- [ ] Payment tolerance and discount, payment methods, payment terms with discount (payment terms with discount, discount on application and unapply done; payment tolerance, VAT adjustment of discounts to do)
- [ ] Intercompany partners, IC inbox and outbox, consolidation
- [ ] Every Finance and Bank report in the inventory

### Phase C: Sales and Purchases complete (Sales 222 / 78, Purchases 153 / 51)

- [ ] **Return orders**: sales and purchase return orders, posted return receipts and shipments
- [ ] **Blanket orders**, quote to order to blanket flows
- [ ] **Prepayments**: Post Prepayment Invoice / Credit Memo, Pending Prepayment status
- [ ] Archive Document and No. of Archived Versions
- [ ] Cancel / Correct posted invoices; Cancelled and Corrective fields
- [ ] Calculate Invoice Discount, customer/vendor invoice discounts
- [ ] Drop shipment and special orders
- [ ] Standard sales/purchase codes, Get Recurring Lines
- [ ] Customer and vendor templates (Apply Template), ship-to and order addresses, customer bank accounts
- [ ] **Incoming documents**: create from file, OCR-less matching, link to journals and documents
- [ ] Sales and purchase analysis reports, salespeople and purchasers, sales budgets
- [ ] Every Sales and Purchases report in the inventory

### Phase D: Pricing (14 / 2)

- [ ] Price lists: sales and purchase prices and discounts, price list lines, Price and Discount Overview
- [ ] Best price on document lines, customer price and discount groups

### Phase E: Inventory (312 / 101)

- [ ] **Item tracking**: lot and serial numbers, item tracking lines, entries, Document Line Tracking (tracking codes, Item Tracking Lines on sales / purchase / journal / transfer lines, split and checked posting, Item Tracking Entries, Lot / Serial No. Information cards with Blocked and must-exist rules, expiration dates (calculated, required, one per lot, strict posting) done; Document Line Tracking, Item Tracing to do)
- [x] **Reservations** and Reserved Quantity, order promising (reserve from inventory and purchase orders, cancel, Reservation Entries, receipts move and shipments use reservations; Available-to-Promise dates. Automatic reservation for Reserve = Always to do)
- [x] **Item charges**: assignment, Qty. to Assign (purchase and sales item charges with assignment, Suggest Equally / By Amount, Get Receipt Lines / Get Shipment Lines, cost onto receiving entries, sales amount onto shipped entries; by weight / volume, transfer and return lines to do)
- [x] **Transfer orders**: shipments and receipts between locations (ship / receive through in-transit, partial quantities, direct transfers, posted shipments and receipts)
- [ ] Stockkeeping units, item variants, units of measure per item, item references, substitutions, catalog items (item units of measure with conversion on documents, variants, references, substitutions, extended texts, translations, attributes, categories, stockkeeping units with Create Stockkeeping Unit and planning per SKU, variants on document / journal lines and entries, catalog items with Create Item from item templates done; nonstock items on sales lines to do)
- [x] Physical inventory orders and recordings, counting periods (Calculate Lines, Calculate Counting Period, Copy Document, Calc. Qty. Expected, Make New Recording, finish / reopen recordings and orders, Test Report, Statistics, posting through the phys. inventory journal into posted orders and recordings; by bins, item tracking on recordings, export / import to do)
- [x] Costing: Adjust Cost – Item Entries, Post Inventory Cost to G/L, revaluation journal, standard cost worksheet (value entries, Adjust Cost with moving average and FIFO / LIFO cost layers (Specific as FIFO), Post Inventory Cost to G/L, automatic cost posting, revaluation journal with Calculate Inventory Value, standard cost worksheet with Suggest / Copy / Implement done; item application entries, Specific by serial to do)
- [ ] Requisition worksheet, reordering policies, item availability by period, location, event and BOM level (requisition worksheet with Calculate Plan by reordering policy and Carry Out Action Message, item availability by event / period / location / variant done; by BOM level to do)
- [ ] Item budgets and analysis reports, item attributes and categories
- [ ] Every Inventory report in the inventory

### Phase F: Warehouse (144 / 35)

- [ ] Bins and bin contents, zones (directed put-away and pick)
- [ ] Warehouse receipts, shipments, put-aways, picks, movements
- [ ] Warehouse journals, worksheets, physical inventory by bin
- [ ] Every Warehouse report in the inventory

### Phase G: Assembly and Manufacturing (Assembly 31 / 9, Manufacturing 134 / 50, plus the Manufacturing and Subcontracting apps)

- [ ] Assembly BOMs, assembly orders, assemble-to-order on sales lines
- [ ] Production BOMs, routings, work and machine centers, capacity
- [ ] Production orders (simulated to finished), output and consumption journals
- [ ] Planning worksheet (MRP), production forecasts
- [ ] Subcontracting
- [ ] Every Assembly and Manufacturing report in the inventory

### Phase H: Service Management (192 / 55, plus the ServiceManagement app)

Highly relevant for a workshop.

- [ ] Service items, service contracts and contract quotes
- [ ] Service orders, quotes, invoices and credit memos, service lines and posting
- [ ] Repair status, fault and resolution codes, service hours, dispatching, loaners
- [ ] Every Service report in the inventory

### Phase I: Projects (135 / 40)

- [ ] Projects (jobs), tasks, planning lines, project journals, invoicing from projects
- [ ] WIP calculation and posting, project budgets
- [ ] Time sheets, resources, resource prices and capacity
- [ ] Every Projects report in the inventory

### Phase J: Fixed assets complete (61 / 38)

- [ ] Insurance, maintenance, FA reclassification, budgets, depreciation tables, more depreciation methods
- [ ] Every Fixed Assets report in the inventory

### Phase K: Cash flow and cost accounting (Cash Flow 22 / 3, Cost Accounting 30 / 21)

- [ ] Cash flow forecasts, worksheets, setup
- [ ] Cost types, cost centers and objects, allocations, cost budgets
- [ ] Every report of both areas

### Phase L: CRM and contacts (163 / 36)

- [ ] Contacts and contact persons, business relations, customer/vendor from contact
- [ ] Interactions, opportunities, sales cycles, campaigns, segments, to-dos
- [ ] Email logging, Outlook-free email sending (SMTP)
- [ ] Every CRM report in the inventory

### Phase M: Human resources (57 / 15)

- [ ] Employees, absences, qualifications, employee ledger (expenses to pay employees)
- [ ] Every HR report in the inventory

### Phase N: Platform (System 173 / 15, Foundation 58 / 2, Utilities 12 / 2, Integration 87, Modules 93, OtherCapabilities 30, eServices 25, Invoicing 7, root 44)

- [ ] Number series UI, company information, setups, assisted setup and checklists
- [ ] Users, permission sets and security groups UI, user tasks, approvals admin
- [ ] Change log, data exchange definitions, report layouts and selections
- [ ] Configuration packages (RapidStart), Excel import and export, data archive
- [ ] Job queue (or a deliberate replacement), notifications, email accounts and scenarios
- [ ] Every remaining page and report of these areas in the inventory

### Phase O: Role centers (16)

- [ ] Every role center: Business Manager (exists), Accountant, Sales Order Processor, Purchasing Agent, Warehouse Worker, Production Planner, Project Manager, Service Dispatcher, and the others in the inventory, with their cues, headlines and actions

### Phase P: W1 apps

Each app gets a decision first, then its pages and reports from the inventory.

- [ ] **Relevant for Indonesia and Auto Craft first:**
  - Withholding Tax (PPh);
  - E-Documents and connectors (e-Faktur by its own connector);
  - Bank Deposits;
  - Statistical Accounts;
  - Payment Practices;
  - Subscription Billing;
  - Quality Management;
  - Sustainability;
  - Shopify (online sales);
  - Sales and Inventory Forecast;
  - Excise Taxes;
  - Simplified Bank Statement Import;
  - Excel Reports;
  - Data Archive;
  - Review G/L Entries;
  - Error Messages with Recommendations;
  - Essential Business Headlines;
  - Master Data Management.
- [ ] **Country-specific elsewhere**, built only if a market needs them: Intrastat, SAF-T, Audit File Export, Service Declaration, VAT Group Management, PEPPOL, UK Send Remittance Advice, IN Tax Engine, EU 3-Party Trade.
- [ ] **Microsoft-service apps**, each needing the user's decision: replace it or leave it out.
  - Hybrid/migration (GP, SL, BC14, QuickBooks);
  - API v1/v2 (an ERPCraft REST API instead?);
  - Power BI reports;
  - Copilot and agent apps (Sales Order Agent, Expense Agent, Payables Agent, AI toolkits, Image Analysis, Late Payment Predictor);
  - Azure/SharePoint/SFTP file storage connectors;
  - Microsoft 365 / Outlook email connectors;
  - Universal Print, Field Service, Company Hub, Cross-environment Intercompany;
  - PayPal / WorldPay / MS Wallet payments.

### Phase Q: Look and feel, to the last detail

- [ ] Option captions the page overrides ("Default (No)")
- [ ] Native file picker for attachments and imports
- [ ] "Resize" in the column header menu
- [ ] FactBox tabs (Details / Attachments), Customer Picture, cues inside FactBoxes
- [ ] Tell Me grouped as in BC; "Show more" and "Show less" everywhere BC has them
- [ ] Web build (wasm) checked after every phase
- [ ] The visual checklist below, page by page

## Visual parity checklist

Pages compared side by side with the live client (layout, components, hover, clicks). Every page that lands in a phase is added here.

- [x] Customer Card, Customer List
- [x] Item List, Item Card
- [x] Sales Order (header, lines part, totals)
- [x] Purchase Order
- [x] Posted Sales Invoice
- [ ] Vendor Card, Vendor List
- [ ] Sales Quote, Invoice, Credit Memo and lists
- [ ] Purchase Quote, Invoice, Credit Memo and lists
- [ ] Posted credit memos, shipments, receipts
- [ ] Ledger entry lists (G/L, customer, vendor, item, bank, FA)
- [ ] Journals (general, cash receipt, payment, item, reclass, phys. inventory, FA)
- [ ] Chart of Accounts, G/L Account Card, Bank Account Card and List
- [ ] Fixed Asset Card and List, FA setup
- [ ] Setup pages
- [ ] Role center
- [ ] Dimensions, Comments, Approvals and Attachments pages

## Known differences (to close)

- G/L entries of a posted document carry the header's dimension set, not each line's.
- Default dimension Value Posting rules are not enforced.
- DateTime fields are read-only text.
- Attach a file takes a typed path; BC opens a file dialog.
