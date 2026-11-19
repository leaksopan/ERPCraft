# Printing and list pages

## Printing documents

`document.print {table, key, path?}` renders a document as an A4 PDF, laid out like BC's standard reports:

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

With `path` (must end in `.pdf`), the file is written and the result gives `path` and `pages`. Without a path, the PDF comes back as `pdf_base64`, which is how MCP clients get it.

```sh
erpcraft-cli run document.print '{"table":"Sales Invoice Header","key":"PS-INV103001","path":"invoice.pdf"}'
```

The desktop app's **Print...** action (Ctrl+P) on documents saves to `Documents\ERPCraft\<table> <key>.pdf` and opens it in the system PDF viewer.

The PDF writer (`crates/engine/src/print/pdf.rs`) has no dependencies. It uses the standard Helvetica fonts with WinAnsi encoding, so characters outside Latin-1 print as `?`.

## List pages

- **Filter pane** (⏷, or a list opened with filters): "Filter list by" holds one expression per field, and **+ Filter...** adds a field. Expressions use BC syntax: `10000..20000`, `<>0`, `>100`, `A|B`, `*oil*`, and `''` for blank.
- **Sorting:** click a column header for ascending ▲, again for descending ▼, and a third time for the page's own order.
- **Columns** (⚙): show or hide any field of the table, or reset to the page's columns. The choice lasts for the session.
- **Date picker:** date fields have a 📅 calendar with month navigation (weeks start Monday) and a Today link.
- **Links:** the control channel and `--open` accept `page?Field=expr&@Field` (filter, sort ascending; `@-Field` sorts descending).
