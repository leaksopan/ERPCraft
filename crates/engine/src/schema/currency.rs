//! Currencies: 330 Currency Exchange Rate (page 483), the Currency Card (495) with its gain and
//! loss accounts, the currency factor on documents and journal lines, and Adjust Exchange Rates.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, GroupDef as G, Metadata, PageDef, PageKind, TableDef};

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
