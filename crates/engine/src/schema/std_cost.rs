//! Standard cost worksheet: 5840 Standard Cost Worksheet Name, 5841 Standard Cost Worksheet,
//! pages 5840 Standard Cost Worksheet Names and 5841 Standard Cost Worksheet.

use erpcraft_meta::{ActionArea as A, ActionDef as Act, FieldDef as F, Metadata, PageDef, PageKind, TableDef};

use crate::std_cost::{WORKSHEET, WORKSHEET_NAME};

pub fn register(m: &mut Metadata) {
    if let Some(t) = m.tables.get_mut(crate::schema::tables::ITEM) {
        for f in [F::decimal("Indirect Cost %"), F::decimal("Overhead Rate").auto_format(2)] {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }
    m.add_table(
        TableDef::new(WORKSHEET_NAME, 5840)
            .fields(vec![F::code("Name", 10).not_blank(), F::text("Description", 50)])
            .key(&["Name"])
            .describe_by("Description")
            .pages("standard-cost-worksheet-names", None),
    );
    m.add_table(
        TableDef::new(WORKSHEET, 5841)
            .fields(vec![
                F::code("Standard Cost Worksheet Name", 10).relation(WORKSHEET_NAME),
                F::option("Type", &["Item", "Work Center", "Machine Center", "Resource"]),
                F::code("No.", 20),
                F::text("Description", 100),
                F::boolean("Implemented").read_only(),
                F::option("Replenishment System", &["Purchase", "Prod. Order", "", "Assembly", "Transfer"]).read_only(),
                F::decimal("Standard Cost").auto_format(2).read_only(),
                F::decimal("New Standard Cost").auto_format(2),
                F::decimal("Indirect Cost %").read_only(),
                F::decimal("New Indirect Cost %"),
                F::decimal("Overhead Rate").auto_format(2).read_only(),
                F::decimal("New Overhead Rate").auto_format(2),
            ])
            .key(&["Standard Cost Worksheet Name", "Type", "No."]),
    );
    let mut names = PageDef::new("standard-cost-worksheet-names", 5840, "Standard Cost Worksheet Names", PageKind::List, Some(WORKSHEET_NAME));
    names.columns = ["Name", "Description"].iter().map(|c| (*c).to_string()).collect();
    names.editable = true;
    names.actions = vec![Act::page("Edit Worksheet", A::Process, "standard-cost-worksheet").link("Standard Cost Worksheet Name", "Name").icon("edit").tooltip("Open the related worksheet.")];
    m.add_page(names);
    let mut ws = PageDef::new("standard-cost-worksheet", 5841, "Standard Cost Worksheet", PageKind::List, Some(WORKSHEET)).view("Standard Cost Worksheet Name", "DEFAULT");
    ws.columns = ["Type", "No.", "Description", "Standard Cost", "New Standard Cost", "Indirect Cost %", "New Indirect Cost %", "Overhead Rate", "New Overhead Rate", "Implemented", "Replenishment System"].iter().map(|c| (*c).to_string()).collect();
    ws.editable = true;
    ws.keywords = "standard cost revalue price change".to_string();
    ws.actions = vec![
        Act::command("Suggest I&tem Standard Cost", A::Process, "standard_cost.suggest_item")
            .icon("new")
            .ask(&["item_no", "standard_cost_factor", "rounding_precision"])
            .tooltip("Create suggestions for changing the cost shares of standard costs on items. Note that the suggested changes are not implemented until you choose the Implement Standard Cost Changes action."),
        Act::command("Copy Standard Cost Worksheet", A::Process, "standard_cost.copy").icon("copy").ask(&["from_worksheet"]).tooltip("Copy standard cost worksheets from several sources into the Standard Cost Worksheet window."),
        Act::command("&Implement Standard Cost Changes", A::Process, "standard_cost.implement")
            .icon("post")
            .ask(&["posting_date", "document_no"])
            .tooltip("Update the changes in the standard cost values on the item card. The suggested costs are not active until you implement them. When you implement the standard cost changes, you also create revaluation journal lines that you can post to revalue inventory."),
    ];
    m.add_page(ws);
}
