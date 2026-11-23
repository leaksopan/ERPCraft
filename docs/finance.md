# Finance: journals, applications, reversal, shipping and invoicing, reports

Core double-entry general ledger, sub-ledgers, document posting, and financial statements.

## General journals

Three journal pages, each on its template's `DEFAULT` batch:

| Page | Template | Source code | Number series |
| --- | --- | --- | --- |
| General Journals | `GENERAL` | `GENJNL` | `G-JNL` (G00001…) |
| Cash Receipt Journals | `CASHRCPT` | `CASHRECJNL` | `G-RCPT` (GR00001…) |
| Payment Journals | `PAYMENT` | `PAYMENTJNL` | `G-PMT` (GP00001…) |

When you add a line (`OnInsert`), it gets:
- the next line number and the posting date of the previous line (or the work date);
- a document number: the same as the previous line while that document is unbalanced, otherwise the next number of the batch's series;
- Document Type *Payment* in cash receipt and payment journals;
- the batch's balancing account (the `OPERATING` bank in the demo).

What validating each field does:
- **Account Type / Account No.:** G/L Account, Customer, Vendor or Bank Account, with the account name as description. A G/L account must be a Posting account and not blocked.
- **Debit Amount / Credit Amount / Amount:** each fills the other two.
- **Applies-to Doc. No.:** finds the open customer or vendor entry and, when Amount is empty, fills it with the remaining amount.

Posting is `journal.post {template, batch}`, or **Post** (F9) on the page:
1. **Balance.** Lines are grouped into documents by Document No. and Posting Date. A document must balance unless its lines carry a balancing account. Use `journal.check` to see any problems without posting.
2. **Transactions.** Each document becomes one transaction (Transaction No.) with a G/L Register. Customer and vendor lines go to the receivables or payables account and their ledger. Bank lines go to the bank's G/L account and the bank ledger. A line with Applies-to Doc. No. is applied to that document.
3. **Clean-up.** Posted lines are deleted, and the series moves past the numbers used.

## Detailed entries and applications

Every customer and vendor ledger entry gets an *Initial Entry* detailed entry
(`Detailed Cust. Ledg. Entry` / `Detailed Vendor Ledg. Entry`). Applying writes a pair of
*Application* entries and updates Remaining Amount, Open and Closed by Entry No.

- `customer.entries.apply` / `vendor.entries.apply`: applies an open entry, either to the given entries or document, or by default to the open entries of the opposite sign, oldest due date first. On the ledger entry pages this is **Apply Entries**.
- `customer.entries.unapply` / `vendor.entries.unapply`: undoes the latest application of an entry. It writes reversing *Application* entries marked Unapplied and reopens every entry involved. On the pages this is **Unapply Entries**.
- `payment.customer.register` / `payment.vendor.register` (one-step payments) use the same ledger and application code as journals.

## Reverse Transaction

`transaction.reverse {transaction_no}`, or **Reverse Transaction** on a ledger entry, posts the
opposite of every G/L, customer, vendor and bank entry of a journal transaction. It writes a
G/L Register with source code `REVERSAL` and marks both the original and the new entries Reversed.

These reversals are refused:
- a transaction that is already reversed;
- a reversal itself;
- an entry that is still applied (unapply it first);
- a transaction posted from a sales or purchase document (use a credit memo instead).

## Ship / Receive and Invoice separately

Orders post in two independent steps. Pick one with `mode` (in the UI, the dialog that **Post...** opens on an order):

| Mode | Sales | Purchase |
| --- | --- | --- |
| Ship and Invoice / Receive and Invoice (default) | both | both |
| Ship / Receive | Posted Sales Shipment, item ledger entries | Posted Purchase Receipt, item ledger entries, unit cost |
| Invoice | Posted invoice, G/L (revenue, VAT, COGS), customer ledger | Posted invoice, G/L (inventory, VAT), vendor ledger |

Line quantity invariants:
- Outstanding Quantity = Quantity − Quantity Shipped.
- Qty. to Ship defaults to the outstanding quantity.
- Qty. to Invoice = Quantity Shipped + Qty. to Ship − Quantity Invoiced.
- Qty. Shipped Not Invoiced is kept on the line.
- Posting refuses to invoice more than has been shipped.
- An order is deleted once every line is fully shipped and invoiced.

Invoices and credit memos still post stock and amounts in one step.

## Reports

| Command | Report |
| --- | --- |
| `report.balance_sheet {date}` | Assets, liabilities and equity by account at a date, with the period's net income under equity. Returns `balanced`. |
| `report.income_statement {from, to}` | Revenue, cost of goods sold, gross profit, expenses and net income for a period (default: start of the year to the work date). |
| `report.aged_receivables {date}` | Open customer balances by days past due: Not Due, 1-30, 31-60, 61-90, Over 90. |
| `report.aged_payables {date}` | The same for vendors. |
| `report.trial_balance {date}` | G/L account balances with debit and credit totals. |

In the UI they are under **Reports ▾** on the role center. Balance Sheet and Income Statement are also on the Chart of Accounts, Aged Accounts Receivable on Customers, and Aged Accounts Payable on Vendors.

## Not yet

- VAT on journal lines.
- Payment discounts and payment tolerance.
- Currencies and exchange rates.
- Recurring journals.
- Applying credit memos from the document (Applies-to Doc. No. on the header).
- Return orders with receipts.
- Expected cost posting.
