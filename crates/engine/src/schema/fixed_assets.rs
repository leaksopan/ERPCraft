//! Phase 3, fixed assets (BC FixedAssets): Fixed Asset 5600, FA Depreciation Book 5612, FA Ledger
//! Entry 5601, FA Posting Group 5606, Depreciation Book 5611, FA Class / Subclass / Location,
//! FA Setup 5603; FA lines on the general journal (FA G/L Journal 5628).

use erpcraft_meta::{
    ActionArea as A, ActionDef as Act, FactBoxDef, FieldDef as F, GroupDef as G, Metadata, NavLink as L, PageDef, PageKind as K,
    TableDef,
};

use super::finance::GEN_JNL_LINE;
use super::tables::{GL_ACCOUNT, NO_SERIES, VENDOR};

pub const FIXED_ASSET: &str = "Fixed Asset";
pub const FA_DEPR_BOOK: &str = "FA Depreciation Book";
pub const FA_LEDGER: &str = "FA Ledger Entry";
pub const FA_POSTING_GROUP: &str = "FA Posting Group";
pub const DEPRECIATION_BOOK: &str = "Depreciation Book";
pub const FA_CLASS: &str = "FA Class";
pub const FA_SUBCLASS: &str = "FA Subclass";
pub const FA_LOCATION: &str = "FA Location";
pub const FA_SETUP: &str = "FA Setup";

pub const FA_POSTING_TYPES: &[&str] =
    &["Acquisition Cost", "Depreciation", "Write-Down", "Appreciation", "Proceeds on Disposal", "Gain/Loss", "Book Value on Disposal"];
/// The journal's FA Posting Type (BC "Gen. Journal Line"."FA Posting Type").
pub const JNL_FA_POSTING_TYPES: &[&str] = &["", "Acquisition Cost", "Depreciation", "Write-Down", "Disposal"];
pub const DEPRECIATION_METHODS: &[&str] = &["Straight-Line", "Declining-Balance 1", "Manual"];

pub fn register(m: &mut Metadata) {
    tables(m);
    pages(m);
    navigation(m);
}

fn tables(m: &mut Metadata) {
    let code_name = |name: &str, id: u32, page: &str| {
        TableDef::new(name, id)
            .fields(vec![F::code("Code", 10).not_blank(), F::text("Name", 50)])
            .key(&["Code"])
            .describe_by("Name")
            .pages(page, None)
    };
    m.add_table(code_name(FA_CLASS, 5607, "fa-classes"));
    m.add_table(code_name(FA_LOCATION, 5609, "fa-locations"));
    m.add_table(
        TableDef::new(FA_SUBCLASS, 5608)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::text("Name", 50),
                F::code("FA Class Code", 10).relation(FA_CLASS),
                F::code("Default FA Posting Group", 20).relation(FA_POSTING_GROUP),
            ])
            .key(&["Code"])
            .describe_by("Name")
            .pages("fa-subclasses", None),
    );
    m.add_table(
        TableDef::new(FA_POSTING_GROUP, 5606)
            .fields(vec![
                F::code("Code", 20).not_blank(),
                F::code("Acquisition Cost Account", 20).relation(GL_ACCOUNT),
                F::code("Accum. Depreciation Account", 20).relation(GL_ACCOUNT),
                F::code("Write-Down Account", 20).relation(GL_ACCOUNT),
                F::code("Depreciation Expense Acc.", 20).relation(GL_ACCOUNT),
                F::code("Write-Down Expense Acc.", 20).relation(GL_ACCOUNT),
                F::code("Gains Acc. on Disposal", 20).relation(GL_ACCOUNT),
                F::code("Losses Acc. on Disposal", 20).relation(GL_ACCOUNT),
            ])
            .key(&["Code"])
            .pages("fa-posting-groups", None),
    );
    m.add_table(
        TableDef::new(DEPRECIATION_BOOK, 5611)
            .fields(vec![
                F::code("Code", 10).not_blank(),
                F::text("Description", 100),
                F::boolean("G/L Integration - Acq. Cost"),
                F::boolean("G/L Integration - Depreciation"),
                F::boolean("G/L Integration - Disposal"),
                F::boolean("Fiscal Year 365 Days"),
            ])
            .key(&["Code"])
            .describe_by("Description")
            .pages("depreciation-books", None),
    );
    m.add_table(
        TableDef::new(FA_SETUP, 5603)
            .fields(vec![
                F::code("Primary Key", 10),
                F::code("Default Depr. Book", 10).relation(DEPRECIATION_BOOK),
                F::code("Fixed Asset Nos.", 20).relation(NO_SERIES),
            ])
            .key(&["Primary Key"])
            .pages("fa-setup", Some("fa-setup")),
    );
    m.add_table(
        TableDef::new(FIXED_ASSET, 5600)
            .fields(vec![
                F::code("No.", 20),
                F::text("Description", 100),
                F::code("Search Description", 100).read_only(),
                F::text("Description 2", 50),
                F::code("FA Class Code", 10).relation(FA_CLASS),
                F::code("FA Subclass Code", 10).relation(FA_SUBCLASS),
                F::code("FA Location Code", 10).relation(FA_LOCATION),
                F::code("FA Posting Group", 20).relation(FA_POSTING_GROUP),
                F::code("Vendor No.", 20).relation(VENDOR),
                F::code("Maintenance Vendor No.", 20).relation(VENDOR),
                F::text("Serial No.", 50),
                F::code("Responsible Employee", 20),
                F::date("Warranty Date"),
                F::boolean("Blocked"),
                F::boolean("Inactive"),
                F::boolean("Acquired").read_only(),
                F::date("Last Date Modified").read_only(),
                F::decimal("Acquisition Cost").flow(),
                F::decimal("Depreciation").flow(),
                F::decimal("Book Value").flow(),
            ])
            .key(&["No."])
            .describe_by("Description")
            .pages("fixed-assets", Some("fixed-asset-card")),
    );
    m.add_table(
        TableDef::new(FA_DEPR_BOOK, 5612)
            .fields(vec![
                F::code("FA No.", 20).relation(FIXED_ASSET),
                F::code("Depreciation Book Code", 10).relation(DEPRECIATION_BOOK),
                F::option("Depreciation Method", DEPRECIATION_METHODS),
                F::date("Depreciation Starting Date"),
                F::decimal("No. of Depreciation Years"),
                F::date("Depreciation Ending Date"),
                F::decimal("Declining-Balance %"),
                F::code("FA Posting Group", 20).relation(FA_POSTING_GROUP),
                F::date("Acquisition Date").read_only(),
                F::date("Disposal Date").read_only(),
                F::decimal("Acquisition Cost").flow(),
                F::decimal("Depreciation").flow(),
                F::decimal("Book Value").flow(),
                F::decimal("Proceeds on Disposal").flow(),
                F::decimal("Gain/Loss").flow(),
                F::date("Last Depreciation Date").flow(),
            ])
            .key(&["FA No.", "Depreciation Book Code"]),
    );
    m.add_table(
        TableDef::new(FA_LEDGER, 5601)
            .fields(vec![
                F::integer("Entry No."),
                F::code("FA No.", 20).relation(FIXED_ASSET),
                F::code("Depreciation Book Code", 10),
                F::date("FA Posting Date"),
                F::date("Posting Date"),
                F::option("Document Type", super::tables::LEDGER_DOC_TYPES),
                F::code("Document No.", 20),
                F::code("External Document No.", 35),
                F::text("Description", 100),
                F::option("FA Posting Type", FA_POSTING_TYPES),
                F::decimal("Amount"),
                F::decimal("Debit Amount"),
                F::decimal("Credit Amount"),
                F::integer("No. of Depreciation Days"),
                F::code("FA Posting Group", 20),
                F::code("FA Location Code", 10),
                F::boolean("Part of Book Value"),
                F::integer("Transaction No."),
                F::code("Source Code", 10),
                F::code("Journal Batch Name", 10),
                F::code("User ID", 50),
            ])
            .key(&["Entry No."])
            .describe_by("Description")
            .pages("fa-ledger-entries", None),
    );
    // FA lines on the general journal.
    if let Some(t) = m.tables.get_mut(GEN_JNL_LINE) {
        t.fields.extend([
            F::option("FA Posting Type", JNL_FA_POSTING_TYPES),
            F::code("Depreciation Book Code", 10).relation(DEPRECIATION_BOOK),
            F::integer("No. of Depreciation Days").read_only(),
        ]);
    }
}

fn pages(m: &mut Metadata) {
    m.add_page(
        PageDef::new("fixed-assets", 5601, "Fixed Assets", K::List, Some(FIXED_ASSET))
            .card("fixed-asset-card")
            .read_only()
            .columns(&["No.", "Description", "FA Class Code", "FA Subclass Code", "FA Location Code", "Acquired", "Acquisition Cost", "Book Value", "Vendor No."])
            .factbox(FactBoxDef::new("Fixed Asset Statistics", None, &["No.", "Acquisition Cost", "Depreciation", "Book Value"]))
            .action(Act::page("Ledger E&ntries", A::Navigate, "fa-ledger-entries").link("FA No.", "No.").shortcut("Ctrl+F7").icon("ledger"))
            .action(Act::command("Calculate Depreciation", A::Process, "fa.calculate_depreciation").ask(&["date", "post"]).icon("stats"))
            .action(Act::command("Fixed Asset Book Value", A::Report, "report.fa_book_value"))
            .keywords("fixed asset aset tetap equipment vehicle"),
    );
    m.add_page(
        PageDef::new("fixed-asset-card", 5600, "Fixed Asset Card", K::Card, Some(FIXED_ASSET))
            .group(
                G::new(
                    "General",
                    &["No.", "Description", "Description 2", "FA Class Code", "FA Subclass Code", "FA Location Code", "Serial No.", "Responsible Employee", "Blocked", "Inactive", "Acquired", "Last Date Modified"],
                )
                .promoted(&["No.", "Description", "FA Subclass Code"]),
            )
            .part("Depreciation Book", "fa-depreciation-books", &[("FA No.", "No.")])
            .group(G::new("Maintenance", &["Vendor No.", "Maintenance Vendor No.", "Warranty Date"]).collapsed())
            .factbox(FactBoxDef::new("Fixed Asset Statistics", None, &["No.", "Acquisition Cost", "Depreciation", "Book Value"]))
            .action(Act::page("Ledger E&ntries", A::Navigate, "fa-ledger-entries").link("FA No.", "No.").shortcut("Ctrl+F7").icon("ledger"))
            .action(Act::command("Calculate Depreciation", A::Process, "fa.calculate_depreciation").ask(&["date", "post"]).icon("stats")),
    );
    m.add_page(
        PageDef::new("fa-depreciation-books", 5619, "Depreciation Books", K::ListPart, Some(FA_DEPR_BOOK)).columns(&[
            "Depreciation Book Code",
            "FA Posting Group",
            "Depreciation Method",
            "Depreciation Starting Date",
            "No. of Depreciation Years",
            "Depreciation Ending Date",
            "Acquisition Cost",
            "Depreciation",
            "Book Value",
            "Last Depreciation Date",
            "Disposal Date",
        ]),
    );
    m.add_page(
        PageDef::new("fa-ledger-entries", 5604, "FA Ledger Entries", K::List, Some(FA_LEDGER))
            .read_only()
            .columns(&["FA Posting Date", "Document Type", "Document No.", "FA No.", "Depreciation Book Code", "FA Posting Type", "Description", "Amount", "No. of Depreciation Days", "Entry No."])
            .keywords("entries fixed asset history"),
    );
    m.add_page(
        PageDef::new("fa-gl-journal", 5628, "Fixed Asset G/L Journals", K::List, Some(GEN_JNL_LINE))
            .columns(&[
                "Posting Date",
                "Document Type",
                "Document No.",
                "Account Type",
                "Account No.",
                "FA Posting Type",
                "Depreciation Book Code",
                "Description",
                "Debit Amount",
                "Credit Amount",
                "Bal. Account Type",
                "Bal. Account No.",
                "No. of Depreciation Days",
            ])
            .view("Journal Template Name", "ASSETS")
            .view("Journal Batch Name", "DEFAULT")
            .action(
                Act::command("Post", A::Home, "journal.post")
                    .param("template", "ASSETS")
                    .param("batch", "DEFAULT")
                    .shortcut("F9")
                    .icon("post")
                    .confirm("Do you want to post the journal lines?"),
            )
            .action(Act::command("Calculate Depreciation", A::Process, "fa.calculate_depreciation").ask(&["date"]).icon("stats"))
            .keywords("fixed asset journal acquisition depreciation disposal hibah"),
    );
    let lists: &[(&str, u32, &str, &str, &[&str])] = &[
        ("fa-classes", 5615, "FA Classes", FA_CLASS, &["Code", "Name"]),
        ("fa-subclasses", 5616, "FA Subclasses", FA_SUBCLASS, &["Code", "Name", "FA Class Code", "Default FA Posting Group"]),
        ("fa-locations", 5617, "FA Locations", FA_LOCATION, &["Code", "Name"]),
        (
            "fa-posting-groups",
            5613,
            "FA Posting Groups",
            FA_POSTING_GROUP,
            &["Code", "Acquisition Cost Account", "Accum. Depreciation Account", "Depreciation Expense Acc.", "Gains Acc. on Disposal", "Losses Acc. on Disposal"],
        ),
        (
            "depreciation-books",
            5611,
            "Depreciation Books",
            DEPRECIATION_BOOK,
            &["Code", "Description", "G/L Integration - Acq. Cost", "G/L Integration - Depreciation", "G/L Integration - Disposal", "Fiscal Year 365 Days"],
        ),
    ];
    for (id, obj, caption, table, cols) in lists {
        m.add_page(PageDef::new(id, *obj, caption, K::List, Some(table)).columns(cols).keywords("fixed asset setup"));
    }
    m.add_page(
        PageDef::new("fa-setup", 5607, "Fixed Asset Setup", K::Card, Some(FA_SETUP))
            .group(G::new("General", &["Default Depr. Book", "Fixed Asset Nos."]))
            .keywords("setup fixed asset"),
    );
}

fn navigation(m: &mut Metadata) {
    let Some(rc) = m.role_centers.get_mut(super::role_center::BUSINESS_MANAGER) else { return };
    let at = rc.sections.iter().position(|s| s.caption == "Setup").unwrap_or(rc.sections.len());
    rc.sections.insert(
        at,
        erpcraft_meta::NavGroup::new(
            "Fixed Assets",
            vec![
                L::new("Fixed Assets", "fixed-assets"),
                L::new("Fixed Asset G/L Journals", "fa-gl-journal"),
                L::new("FA Ledger Entries", "fa-ledger-entries"),
                L::new("FA Classes", "fa-classes"),
                L::new("FA Subclasses", "fa-subclasses"),
                L::new("FA Locations", "fa-locations"),
                L::new("FA Posting Groups", "fa-posting-groups"),
                L::new("Depreciation Books", "depreciation-books"),
                L::new("Fixed Asset Setup", "fa-setup"),
            ],
        ),
    );
    if let Some(g) = rc.action_groups.iter_mut().find(|g| g.caption == "Reports") {
        g.links.push(L::command("Fixed Asset Book Value", "report.fa_book_value"));
    }
    if let Some(g) = rc.action_groups.iter_mut().find(|g| g.caption == "New") {
        g.links.push(L::new("Fixed Asset", "fixed-asset-card"));
    }
}
