//! ERPCraft metadata: declarative table, page, and role-center definitions as plain Rust data.
//!
//! - [`TableDef`] / [`FieldDef`] / [`FieldType`]: relational table schema definitions.
//! - [`PageDef`]: metadata-driven UI layouts (List, Card, Document, ListPart, …).
//! - [`RoleCenterDef`]: the home page of a profile (navigation menus, actions, cues).
//! - [`Value`] / [`Record`]: one typed field value and one row.
//!
//! Everything here is data. The engine owns behaviour (triggers, posting); every client (egui UI,
//! CLI, MCP) renders and drives the same metadata, so a new table or page shows up everywhere at
//! once.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unimplemented, clippy::unreachable)]

mod page;
mod record;
mod table;
mod value;

pub use page::{
    ActionArea, ActionDef, CueDef, CueGroupDef, FactBoxDef, GroupDef, NavGroup, NavLink, PageDef, PageKind, PartDef,
    RoleCenterDef, ViewFilter,
};
pub use record::{Key, Record};
pub use table::{CalcFormula, CalcFunc, DrillDown, FieldClass, FieldDef, FieldType, TableDef};
pub use value::{Value, format_decimal};

use std::collections::BTreeMap;

/// Every table, page and role center the application knows, looked up by name / id.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub tables: BTreeMap<String, TableDef>,
    pub pages: BTreeMap<String, PageDef>,
    pub role_centers: BTreeMap<String, RoleCenterDef>,
}

impl Metadata {
    pub fn table(&self, name: &str) -> Option<&TableDef> {
        self.tables.get(name).or_else(|| self.tables.values().find(|t| t.name.eq_ignore_ascii_case(name)))
    }

    pub fn page(&self, id: &str) -> Option<&PageDef> {
        self.pages.get(id).or_else(|| {
            self.pages.values().find(|p| p.id.eq_ignore_ascii_case(id) || p.object_id.to_string() == id)
        })
    }

    pub fn role_center(&self, id: &str) -> Option<&RoleCenterDef> {
        self.role_centers.get(id)
    }

    pub fn add_table(&mut self, t: TableDef) {
        self.tables.insert(t.name.clone(), t);
    }

    pub fn add_page(&mut self, p: PageDef) {
        self.pages.insert(p.id.clone(), p);
    }

    pub fn add_role_center(&mut self, r: RoleCenterDef) {
        self.role_centers.insert(r.id.clone(), r);
    }

    /// Problems in the metadata itself: pages naming unknown tables or fields, parts naming unknown
    /// pages. Run by the engine's tests so a typo in a page definition fails CI, not the UI.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in self.pages.values() {
            let Some(tname) = &p.source_table else { continue };
            let Some(t) = self.table(tname) else {
                out.push(format!("page {}: unknown source table {tname}", p.id));
                continue;
            };
            let fields = p.groups.iter().flat_map(|g| g.fields.iter()).chain(p.columns.iter());
            for f in fields {
                if t.field(f).is_none() {
                    out.push(format!("page {}: table {} has no field {f}", p.id, t.name));
                }
            }
            for f in &p.view {
                if t.field(&f.field).is_none() {
                    out.push(format!("page {}: view filter on unknown field {}", p.id, f.field));
                }
            }
            for part in &p.parts {
                if self.page(&part.page).is_none() {
                    out.push(format!("page {}: part {} names unknown page {}", p.id, part.caption, part.page));
                }
            }
            if let Some(card) = &p.card_page
                && self.page(card).is_none()
            {
                out.push(format!("page {}: unknown card page {card}", p.id));
            }
        }
        for t in self.tables.values() {
            for f in &t.fields {
                if let Some(rel) = &f.relation
                    && self.table(rel).is_none()
                {
                    out.push(format!("table {}: field {} relates to unknown table {rel}", t.name, f.name));
                }
            }
        }
        for rc in self.role_centers.values() {
            let links = rc
                .embedding
                .iter()
                .chain(rc.creation.iter())
                .chain(rc.sections.iter().flat_map(|s| s.links.iter()))
                .chain(rc.action_groups.iter().flat_map(|s| s.links.iter()));
            for l in links.filter(|l| l.command.is_none()) {
                if self.page(&l.page).is_none() {
                    out.push(format!("role center {}: link {} names unknown page {}", rc.id, l.caption, l.page));
                }
            }
        }
        out
    }
}
