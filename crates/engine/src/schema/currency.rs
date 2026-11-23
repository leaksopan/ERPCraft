//! Currencies: 330 Currency Exchange Rate (page 483), the Currency Card (495) with its gain and
//! loss accounts, the currency factor on documents and journal lines, and Adjust Exchange Rates.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, CalcFormula, CalcFunc, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

pub const EXCH_RATE: &str = "Currency Exchange Rate";

fn ensure(m: &mut Metadata, table: &str, fields: Vec<F>) {
    if let Some(t) = m.tables.get_mut(table) {
        for f in fields {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
}

pub fn register(m: &mut Metadata) {
    ensure(
        m,
        "Currency",
        vec![
            F::code("Unrealized Gains Acc.", 20).relation("G/L Account"),
            F::code("Realized Gains Acc.", 20).relation("G/L Account"),
            F::code("Unrealized Losses Acc.", 20).relation("G/L Account"),
            F::code("Realized Losses Acc.", 20).relation("G/L Account"),
            F::date("Last Date Adjusted").read_only(),
        ],
    );
    ensure(
        m,
        crate::schema::tables::CUST_LEDGER,
        vec![
            F::decimal("Remaining Amt. (LCY)")
                .calc(CalcFormula::new(CalcFunc::Sum, crate::schema::finance::DTLD_CUST_LEDGER, "Amount (LCY)", &[("Cust. Ledger Entry No.", "Entry No.")], &[]))
                .auto_format(1),
        ],
    );
    ensure(
        m,
        crate::schema::tables::VEND_LEDGER,
        vec![
            F::decimal("Remaining Amt. (LCY)")
                .calc(CalcFormula::new(CalcFunc::Sum, crate::schema::finance::DTLD_VEND_LEDGER, "Amount (LCY)", &[("Vendor Ledger Entry No.", "Entry No.")], &[]))
                .auto_format(1),
        ],
    );
    if let Some(t) = m.tables.get_mut("Currency") {
        t.card_page = Some("currency-card".to_string());
    }
    for header in ["Sales Header", "Purchase Header"] {
        ensure(m, header, vec![F::decimal("Currency Factor").read_only()]);
    }
    ensure(
        m,
        "Gen. Journal Line",
        vec![F::code("Currency Code", 10).relation("Currency"), F::decimal("Currency Factor").read_only(), F::decimal("Amount (LCY)").read_only().auto_format(1)],
    );
    // Recurring journals: methods, frequency, expiry, and the allocations that balance a line.
    ensure(
        m,
        "Gen. Journal Line",
        vec![
            F::option("Recurring Method", &["", "F  Fixed", "V  Variable", "B  Balance", "RF Reversing Fixed", "RV Reversing Variable", "RB Reversing Balance"]),
            F::code("Recurring Frequency", 20),
            F::date("Expiration Date"),
        ],
    );
    ensure(m, "Gen. Journal Template", vec![F::boolean("Recurring")]);
    m.add_table(
        TableDef::new(crate::recurring::ALLOCATION, 221)
            .fields(vec![
                F::code("Journal Template Name", 10),
                F::code("Journal Batch Name", 10),
                F::integer("Journal Line No."),
                F::integer("Line No."),
                F::code("Account No.", 20).relation("G/L Account"),
                F::text("Account Name", 100).read_only(),
                F::decimal("Allocation Quantity"),
                F::decimal("Allocation %"),
                F::decimal("Amount").auto_format(1).read_only(),
            ])
            .key(&["Journal Template Name", "Journal Batch Name", "Journal Line No.", "Line No."]),
    );
    let mut alloc = PageDef::new("allocations", 284, "Allocations", PageKind::List, Some(crate::recurring::ALLOCATION));
    alloc.columns = ["Account No.", "Account Name", "Allocation Quantity", "Allocation %", "Amount"].iter().map(|c| (*c).to_string()).collect();
    alloc.editable = true;
    m.add_page(alloc);
    if let Some(p) = m.pages.get_mut("recurring-general-journal") {
        p.actions.push(
            Act::page("Allocations", A::Navigate, "allocations")
                .link("Journal Template Name", "Journal Template Name")
                .link("Journal Batch Name", "Journal Batch Name")
                .link("Journal Line No.", "Line No.")
                .icon("list")
                .shortcut("Shift+Ctrl+A")
                .tooltip("Allocate the amount on the selected journal line to the accounts that you specify."),
        );
        p.keywords = "recurring journal allocations accruals".to_string();
    }
    // Payment discounts: terms, documents, entries and the posting groups' accounts.
    ensure(m, "Payment Terms", vec![F::code("Discount Date Calculation", 20), F::decimal("Discount %")]);
    for header in ["Sales Header", "Purchase Header"] {
        ensure(m, header, vec![F::decimal("Payment Discount %"), F::date("Pmt. Discount Date")]);
    }
    for t in [crate::schema::tables::CUST_LEDGER, crate::schema::tables::VEND_LEDGER] {
        ensure(m, t, vec![F::date("Pmt. Discount Date"), F::decimal("Original Pmt. Disc. Possible").read_only().auto_format(1), F::decimal("Remaining Pmt. Disc. Possible").auto_format(1)]);
    }
    for g in ["Customer Posting Group", "Vendor Posting Group"] {
        ensure(m, g, vec![F::code("Payment Disc. Debit Acc.", 20).relation("G/L Account"), F::code("Payment Disc. Credit Acc.", 20).relation("G/L Account")]);
    }
    for t in [crate::schema::finance::DTLD_CUST_LEDGER, crate::schema::finance::DTLD_VEND_LEDGER] {
        ensure(m, t, vec![F::decimal("Debit Amount (LCY)"), F::decimal("Credit Amount (LCY)")]);
    }
    m.add_table(
        TableDef::new(EXCH_RATE, 330)
            .fields(vec![
                F::code("Currency Code", 10).relation("Currency"),
                F::date("Starting Date"),
                F::decimal("Exchange Rate Amount"),
                F::decimal("Adjustment Exch. Rate Amount"),
                F::code("Relational Currency Code", 10).relation("Currency"),
                F::decimal("Relational Exch. Rate Amount"),
                F::option("Fix Exchange Rate Amount", &["Currency", "Relational Currency", "Both"]),
                F::decimal("Relational Adjmt Exch Rate Amt"),
            ])
            .key(&["Currency Code", "Starting Date"]),
    );
    let mut rates = PageDef::new("currency-exchange-rates", 483, "Currency Exchange Rates", PageKind::List, Some(EXCH_RATE));
    rates.columns = ["Starting Date", "Currency Code", "Relational Currency Code", "Exchange Rate Amount", "Relational Exch. Rate Amount", "Adjustment Exch. Rate Amount", "Relational Adjmt Exch Rate Amt", "Fix Exchange Rate Amount"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    rates.editable = true;
    m.add_page(rates);

    let adjust = Act::command("Adjust Exchange Rates", A::Process, "currency.adjust_exchange_rates")
        .icon("adjust")
        .ask(&["ending_date", "document_no"])
        .tooltip("Adjust general ledger, customer, vendor, and bank account entries to reflect a more updated balance if the exchange rate has changed since the entries were posted.");
    let rates_link = Act::page("Exch. &Rates", A::Navigate, "currency-exchange-rates")
        .link("Currency Code", "Code")
        .icon("currency")
        .tooltip("View updated exchange rates for the currencies that you use.");
    if let Some(p) = m.pages.get_mut("currencies") {
        p.card_page = Some("currency-card".to_string());
        p.actions.push(rates_link.clone());
        p.actions.push(adjust.clone());
    }
    m.add_page(
        PageDef::new("currency-card", 495, "Currency Card", PageKind::Card, Some("Currency"))
            .group(G::new("General", &["Code", "Description", "Symbol"]))
            .group(G::new("Posting", &["Unrealized Gains Acc.", "Realized Gains Acc.", "Unrealized Losses Acc.", "Realized Losses Acc."]))
            .action(rates_link)
            .action(adjust),
    );
}
