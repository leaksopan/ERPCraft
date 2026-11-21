//! The base application's metadata: tables, pages and role centers.

mod bc_gen;
pub mod approvals;
pub mod attachments;
pub mod comments;
pub mod dimensions;
pub mod fin_reports;
pub mod fiscal;
pub mod currency;
pub mod vat;
pub mod bank_rec;
pub mod reminders;
pub mod item_setup;
pub mod finance;
pub mod fixed_assets;
pub mod inventory;
pub mod pages;
pub mod role_center;
pub mod tables;

use std::sync::OnceLock;

use erpcraft_meta::Metadata;

/// The base application, built once: the hand-written schema with the fields and page layouts
/// generated from BCApps (`bc_gen.rs`) on top.
pub fn base_app() -> &'static Metadata {
    static META: OnceLock<Metadata> = OnceLock::new();
    META.get_or_init(|| {
        let mut m = hand_written();
        bc_gen::extend(&mut m);
        m
    })
}

/// The schema without the generated layer; `bc-parity gen` generates against this.
pub fn hand_written() -> Metadata {
    let mut m = Metadata::default();
    tables::register(&mut m);
    pages::register(&mut m);
    role_center::register(&mut m);
    finance::register(&mut m);
    fin_reports::register(&mut m);
    fiscal::register(&mut m);
    currency::register(&mut m);
    vat::register(&mut m);
    bank_rec::register(&mut m);
    reminders::register(&mut m);
    item_setup::register(&mut m);
    inventory::register(&mut m);
    fixed_assets::register(&mut m);
    dimensions::register(&mut m);
    comments::register(&mut m);
    approvals::register(&mut m);
    attachments::register(&mut m);
    crate::bc::register(&mut m);
    m
}

/// Helpers the generated `bc_gen.rs` calls.
mod gen_support {
    use erpcraft_meta::{ActionArea, ActionDef, CalcFormula, DrillDown, FieldDef, GroupDef, Metadata};

    /// Appends the fields a table doesn't have yet; hand-written definitions win.
    pub fn fields(m: &mut Metadata, table: &str, fields: Vec<FieldDef>) {
        let Some(t) = m.tables.get_mut(table) else { return };
        for f in fields {
            if t.field(&f.name).is_none() {
                t.fields.push(f);
            }
        }
    }

    /// Hand-written fields get BC's caption, its number format and, where they have none, its
    /// tooltip.
    pub fn relabel(m: &mut Metadata, table: &str, labels: &[(&str, &str, &str, u8)]) {
        let Some(t) = m.tables.get_mut(table) else { return };
        for (name, caption, tooltip, auto_format) in labels {
            if let Some(f) = t.fields.iter_mut().find(|f| f.name == *name) {
                f.caption = (*caption).to_string();
                f.auto_format = *auto_format;
                if f.tooltip.is_empty() {
                    f.tooltip = (*tooltip).to_string();
                }
            }
        }
    }

    /// One FastTab of a generated layout.
    #[derive(Default)]
    pub struct Tab {
        pub caption: &'static str,
        pub fields: &'static [&'static str],
        pub promoted: &'static [&'static str],
        pub additional: &'static [&'static str],
        pub captions: &'static [(&'static str, &'static str)],
        pub headings: &'static [(&'static str, &'static str)],
        pub breaks: &'static [&'static str],
    }

    /// Replaces a card's FastTabs with BC's layout, keeping which tabs start collapsed.
    pub fn layout(m: &mut Metadata, page: &str, tabs: &[Tab]) {
        let Some(p) = m.pages.get_mut(page) else { return };
        let strs = |s: &[&str]| s.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        let pairs = |s: &[(&str, &str)]| s.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect::<Vec<_>>();
        let groups = tabs
            .iter()
            .map(|t| {
                let collapsed = p.groups.iter().find(|g| g.caption.eq_ignore_ascii_case(t.caption)).is_some_and(|g| g.collapsed);
                let mut g = GroupDef::new(t.caption, t.fields);
                g.promoted = strs(t.promoted);
                g.additional = strs(t.additional);
                g.captions = pairs(t.captions);
                g.headings = pairs(t.headings);
                g.breaks = strs(t.breaks);
                g.collapsed = collapsed;
                g
            })
            .collect();
        p.groups = groups;
    }

    /// A navigation action of BC's page: opens `page` filtered by (target field, this record's field).
    pub struct Nav {
        pub caption: &'static str,
        pub page: &'static str,
        pub link: &'static [(&'static str, &'static str)],
        pub shortcut: &'static str,
    }

    /// Adds BC's navigation actions the page doesn't have yet (by caption).
    pub fn actions(m: &mut Metadata, page: &str, navs: &[Nav]) {
        let Some(p) = m.pages.get_mut(page) else { return };
        for n in navs {
            let norm = |s: &str| s.replace('&', "").to_ascii_lowercase();
            if p.actions.iter().any(|a| norm(&a.caption) == norm(n.caption)) {
                continue;
            }
            let icon = if n.caption.contains("Entries") || n.caption.contains("E&ntries") { "ledger" } else { "list" };
            let mut a = ActionDef::page(n.caption, ActionArea::Navigate, n.page).icon(icon);
            for (target, source) in n.link {
                a = a.link(target, source);
            }
            if !n.shortcut.is_empty() {
                a = a.shortcut(n.shortcut);
            }
            p.actions.push(a);
        }
    }

    /// A list page takes its card's actions with these captions (BC offers them on the list too,
    /// acting on the selected row).
    pub fn mirror(m: &mut Metadata, list: &str, card: &str, captions: &[&str]) {
        let norm = |s: &str| s.replace('&', "").trim_end_matches("...").trim().to_ascii_lowercase();
        let Some(source) = m.pages.get(card).map(|c| c.actions.clone()) else { return };
        let Some(p) = m.pages.get_mut(list) else { return };
        for c in captions {
            if p.actions.iter().any(|a| norm(&a.caption) == norm(c)) {
                continue;
            }
            if let Some(a) = source.iter().find(|a| norm(&a.caption) == norm(c)) {
                p.actions.push(a.clone());
            }
        }
    }

    /// The action bar's categories, as BC's page promotes them.
    pub fn categories(m: &mut Metadata, page: &str, cats: &[(&str, &[&str])]) {
        if let Some(p) = m.pages.get_mut(page) {
            p.categories = cats.iter().map(|(c, a)| ((*c).to_string(), a.iter().map(|x| (*x).to_string()).collect())).collect();
        }
    }

    /// A FlowField's drill-down: field, target page, (target, own field) links, (target, filter) fixed.
    pub type Drill = (&'static str, &'static str, &'static [(&'static str, &'static str)], &'static [(&'static str, &'static str)]);

    /// Sets the drill-downs of a table's FlowFields.
    pub fn drilldowns(m: &mut Metadata, table: &str, drills: &[Drill]) {
        let Some(t) = m.tables.get_mut(table) else { return };
        let pairs = |s: &[(&str, &str)]| s.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect::<Vec<_>>();
        for (field, page, link, fixed) in drills {
            if let Some(f) = t.fields.iter_mut().find(|f| f.name == *field) {
                f.drilldown = Some(DrillDown { page: (*page).to_string(), link: pairs(link), fixed: pairs(fixed) });
            }
        }
    }

    /// Gives hand-written FlowFields BC's CalcFormula (hand-written calculations still win).
    pub fn calcs(m: &mut Metadata, table: &str, formulas: Vec<(&str, CalcFormula)>) {
        let Some(t) = m.tables.get_mut(table) else { return };
        for (field, c) in formulas {
            if let Some(f) = t.fields.iter_mut().find(|f| f.name == field && f.calc.is_none()) {
                f.calc = Some(c);
            }
        }
    }

    /// Replaces a list page's columns with BC's.
    pub fn columns(m: &mut Metadata, page: &str, columns: &[&str]) {
        if let Some(p) = m.pages.get_mut(page) {
            p.columns = columns.iter().map(|x| (*x).to_string()).collect();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn metadata_is_consistent() {
        let problems = super::base_app().check();
        assert!(problems.is_empty(), "metadata problems:\n{}", problems.join("\n"));
    }
}
