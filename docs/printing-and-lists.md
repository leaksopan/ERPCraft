# Printing and list pages

## Printing documents

`document.print {table, key, path?}` renders a document as an A4 PDF with standard business document sections:

- company block and the document title
- document details and the bill-to / pay-to address
- lines, totals with VAT (PPN)
- payment terms and the company's bank account

| Table | Document |
| --- | --- |
| Sales Header | Quote, Order, Invoice, Credit Memo |
| Sales Invoice Header, Sales Cr.Memo Header | Posted sales invoice / credit memo |
| Sales Shipment Header | Posted shipment |
| Purchase Header | Quote, Order, Invoice, Credit Memo |
| Purch. Inv. Header, Purch. Cr. Memo Hdr. | Posted purchase invoice / credit memo |
| Purch. Rcpt. Header | Posted receipt |

With `path` (must end in `.pdf`), the file is written (subject to `--automation-write-root` when confined) and the result gives `path` and `pages`. Without a path, the PDF comes back as `pdf_base64`, which is how MCP clients get it.

```sh
erpcraft-cli run document.print '{"table":"Sales Invoice Header","key":"PS-INV103001","path":"invoice.pdf"}'
```

The desktop app's **Print...** action (Ctrl+P) on documents saves to `Documents/ERPCraft/<table> <key>.pdf` and opens it in the system PDF viewer; the web build triggers a browser download via `craft-platform::browser`.

The PDF writer (`crates/engine/src/print/pdf.rs`) has no dependencies. It uses the standard Helvetica fonts with WinAnsi encoding, so characters outside Latin-1 print as `?`.

On native desktop/CLI builds, `print.printers` lists system printer queues via `craft-print`, and `document.print_system {table, key, printer?, copies?, duplex?, grayscale?, scaling?, dry_run?}` renders the PDF and submits it directly to the OS print spooler (Windows XPS / CUPS).

## Data export and import (CSV & Excel)

All tables and list pages support structured export and transactional import via `craft-csv` and `craft-xlsx`:

- `data.export_csv {table|page, filters?, search?, fields?, delimiter?, path?}` — writes an RFC-4180 CSV file (or returns inline `csv` text).
- `data.export_xlsx {table|page, filters?, search?, fields?, sheet_name?, path?}` — writes an `.xlsx` workbook (or returns `xlsx_base64`).
- `data.import_csv {table, path|csv, delimiter?, dry_run?}` — auto-detects encoding and delimiter, validates every row through `records::create`, and rolls back completely if any row fails.
- `data.import_xlsx {table, path|xlsx_base64, sheet?, dry_run?}` — reads a worksheet and inserts rows transactionally.

When running confined (RFC-0015), `path` is checked against `--automation-read-root` (imports) and `--automation-write-root` (exports).

## List pages

- **Filter pane** (⏷, or a list opened with filters): "Filter list by" holds one expression per field, and **+ Filter...** adds a field. Expressions support ranges and operators: `10000..20000`, `<>0`, `>100`, `A|B`, `*oil*`, and `''` for blank.
- **Sorting:** click a column header for ascending ▲, again for descending ▼, and a third time for the page's own order.
- **Columns** (⚙): show or hide any field of the table, or reset to the page's columns. The choice lasts for the session.
- **Date picker:** date fields have a 📅 calendar with month navigation (weeks start Monday) and a Today link.
- **Links:** the control channel and `--open` accept `page?Field=expr&@Field` (filter, sort ascending; `@-Field` sorts descending).
