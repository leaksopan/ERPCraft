//! Writes `crates/engine/src/schema/bc_gen.rs`: the table fields and page layouts of BC's AL
//! sources for every hand-written page that carries a BC object id.
//!
//! It always starts from `schema::hand_written()`, so rerunning it gives the same file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use erpcraft_engine::meta::{Metadata, PageKind};

use crate::al::{self, AlPage, AlTable, AlTableField, FieldClass};

pub struct Sources {
    pub pages: BTreeMap<u32, PathBuf>,
    pub page_ids: BTreeMap<String, u32>,
    pub tables: BTreeMap<u32, PathBuf>,
    pub enums: BTreeMap<String, Vec<String>>,
}

impl Sources {
    pub fn load(root: &Path) -> Self {
        let enums = al::index(root, "Enum", "enum")
            .values()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|s| al::al_enum(&s))
            .collect();
        let pages = al::index(root, "Page", "page");
        let page_ids = al::names(&pages);
        Self { pages, page_ids, tables: al::index(root, "Table", "table"), enums }
    }

    pub fn page(&self, id: u32) -> Option<AlPage> {
        std::fs::read_to_string(self.pages.get(&id)?).ok().and_then(|s| al::al_page(&s))
    }

    pub fn table(&self, id: u32) -> Option<AlTable> {
        std::fs::read_to_string(self.tables.get(&id)?).ok().and_then(|s| al::al_table(&s))
    }
}

/// A CalcFormula as a Rust expression, resolved against the fields that exist after generation
/// (`has(table, field)`). None when its table, value field, a record link or a fixed filter's
/// field isn't here; filters on FlowFilter fields (empty by default) are dropped.
fn calc_expr(cf: &al::CalcFormula, own_al: &AlTable, own_table: &str, meta: &Metadata, src: &Sources, has: &dyn Fn(&str, &str) -> bool) -> Option<String> {
    let target = meta.table(&cf.table)?;
    let target_al = src.table(target.object_id);
    let func = match cf.func.as_str() {
        "sum" => "Sum",
        "count" => "Count",
        "exist" => "Exist",
        "lookup" => "Lookup",
        "min" => "Min",
        "max" => "Max",
        "average" => "Average",
        _ => return None,
    };
    if !matches!(func, "Count" | "Exist") && !has(&target.name, &cf.field) {
        return None;
    }
    let mut link = Vec::new();
    let mut fixed = Vec::new();
    for (tf, s) in &cf.clauses {
        match s {
            al::CalcSrc::Field(own) => {
                if own_al.fields.iter().any(|a| a.name.eq_ignore_ascii_case(own) && a.class == FieldClass::FlowFilter) {
                    continue;
                }
                if !has(own_table, own) || !has(&target.name, tf) {
                    return None;
                }
                link.push(format!("({tf:?}, {own:?})"));
            }
            al::CalcSrc::Expr(e) if has(&target.name, tf) => fixed.push(format!("({tf:?}, {e:?})")),
            // A field ERPCraft doesn't store holds its default on every row: a filter that the default
            // passes changes nothing; any other would sum rows BC leaves out, so no formula then.
            al::CalcSrc::Expr(e) => {
                let field = target_al.as_ref().and_then(|t| t.fields.iter().find(|f| f.name.eq_ignore_ascii_case(tf)))?;
                if !default_passes(field, e) {
                    return None;
                }
            }
        }
    }
    Some(format!("CalcFormula::new(CalcFunc::{func}, {:?}, {:?}, &[{}], &[{}])", target.name, cf.field, link.join(", "), fixed.join(", ")))
}

/// Whether a field's default value (false, 0, blank, the first option) passes filter `expr`.
fn default_passes(f: &AlTableField, expr: &str) -> bool {
    let e = expr.trim().trim_matches('\'').to_ascii_lowercase();
    match f.ty.trim().to_ascii_lowercase().as_str() {
        "boolean" => matches!(e.as_str(), "false" | "no" | "0"),
        "decimal" | "integer" | "biginteger" => matches!(e.as_str(), "0" | "''"),
        t if t.starts_with("code") || t.starts_with("text") => e.is_empty() || e == "''",
        "option" => f.option_captions.first().is_some_and(|o| o.trim().eq_ignore_ascii_case(&e) || (o.trim().is_empty() && (e.is_empty() || e == "''"))),
        _ => false,
    }
}

/// The builder expression for an AL field, or None when ERPCraft has no matching type yet. A
/// FlowField needs its formula (`calc`).
fn field_expr(f: &AlTableField, meta: &Metadata, enums: &BTreeMap<String, Vec<String>>, calc: Option<String>) -> Option<String> {
    if f.obsolete || f.class == FieldClass::FlowFilter || (f.class == FieldClass::FlowField && calc.is_none()) {
        return None;
    }
    let name = format!("{:?}", f.name);
    let ty = f.ty.trim();
    let lower = ty.to_ascii_lowercase();
    let len = |s: &str| s.split_once('[').and_then(|(_, r)| r.trim_end_matches(']').trim().parse::<u16>().ok());
    let options = |values: &[String]| format!("&[{}]", values.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", "));
    let mut read_only = !f.editable;
    let mut e = if lower.starts_with("code[") {
        format!("F::code({name}, {})", len(ty)?)
    } else if lower.starts_with("text[") {
        format!("F::text({name}, {})", len(ty)?)
    } else {
        match lower.as_str() {
            "text" => format!("F::text({name}, 2048)"),
            "decimal" => format!("F::decimal({name})"),
            "integer" | "biginteger" | "duration" => format!("F::integer({name})"),
            "boolean" => format!("F::boolean({name})"),
            "date" => format!("F::date({name})"),
            // Shown, not edited: ERPCraft has no time type yet.
            "datetime" | "time" => {
                read_only = true;
                format!("F::text({name}, 30)")
            }
            "dateformula" => format!("F::code({name}, 32)"),
            "option" if !f.option_captions.is_empty() => format!("F::option({name}, {})", options(&f.option_captions)),
            _ if lower.starts_with("enum") => {
                let values = enums.get(&al::unquote(ty[4..].trim()))?;
                format!("F::option({name}, {})", options(values))
            }
            _ => return None,
        }
    };
    if f.caption != f.name {
        let _ = write!(e, ".caption({:?})", f.caption);
    }
    if let Some(rel) = &f.relation
        && meta.table(rel).is_some_and(|t| t.single_code_key().is_some())
    {
        let _ = write!(e, ".relation({rel:?})");
    }
    match calc {
        Some(c) => {
            let _ = write!(e, ".calc({c})");
        }
        None if read_only => e.push_str(".read_only()"),
        None => {}
    }
    if f.auto_format > 0 && lower == "decimal" {
        let _ = write!(e, ".auto_format({})", f.auto_format);
    }
    if !f.tooltip.is_empty() {
        let _ = write!(e, ".tooltip({:?})", f.tooltip);
    }
    Some(e)
}

fn strs(v: &[String]) -> String {
    format!("&[{}]", v.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", "))
}

pub fn generate(meta: &Metadata, src: &Sources) -> String {
    // Fields each table needs: every table field a BC page of ours shows.
    let mut wanted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut pages: Vec<(String, AlPage)> = Vec::new();
    for p in meta.pages.values().filter(|p| p.object_id > 0 && p.object_id < 50000) {
        let (Some(al), Some(table)) = (src.page(p.object_id), p.source_table.clone()) else { continue };
        let shown = al.groups.iter().filter(|g| g.vis != al::Vis::Hidden).flat_map(|g| g.fields.iter()).chain(al.columns.iter());
        wanted.entry(table).or_default().extend(shown.filter(|f| f.shown()).filter_map(|f| f.source.clone()));
        pages.push((p.id.clone(), al));
    }

    let mut out = String::from(
        "//! Generated by `cargo run -p bc-parity -- gen` from the BCApps W1 Base Application. Do not edit:\n\
         //! change the hand-written schema or the generator and run it again.\n\n\
         use erpcraft_meta::{CalcFormula, CalcFunc, FieldDef as F, Metadata};\n\n\
         use super::gen_support::{Nav, Tab, actions, calcs, categories, columns, drilldowns, fields, layout, mirror, relabel, worksheets};\n\n\
         pub fn extend(m: &mut Metadata) {\n",
    );
    // Field names per table after extension, for the layouts below.
    let mut known: BTreeMap<String, BTreeSet<String>> =
        meta.tables.values().map(|t| (t.name.clone(), t.fields.iter().map(|f| f.name.to_ascii_lowercase()).collect())).collect();
    // A field exists after generation when the hand-written table has it or a page wants it.
    let has = |table: &str, field: &str| {
        meta.table(table).is_some_and(|t| t.field(field).is_some())
            || meta.table(table).and_then(|t| wanted.get(&t.name)).is_some_and(|w| w.iter().any(|n| n.eq_ignore_ascii_case(field)))
    };
    // FlowFields generated here, which get drill-downs below like the hand-written ones.
    let mut gen_flows: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // BC's caption per table field, to tell which page captions are overrides.
    let mut captions: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (table, names) in &wanted {
        let Some(t) = meta.table(table) else { continue };
        let Some(al) = src.table(t.object_id) else { continue };
        captions.insert(t.name.clone(), al.fields.iter().map(|f| (f.name.to_ascii_lowercase(), f.caption.clone())).collect());
        let labels: Vec<String> = al
            .fields
            .iter()
            .filter_map(|a| {
                t.field(&a.name)
                    .filter(|h| h.caption != a.caption || (h.tooltip.is_empty() && !a.tooltip.is_empty()) || h.auto_format != a.auto_format)
                    .map(|h| (h, a))
            })
            .map(|(h, a)| format!("            ({:?}, {:?}, {:?}, {}),", h.name, a.caption, a.tooltip, a.auto_format))
            .collect();
        if !labels.is_empty() {
            let _ = writeln!(out, "    relabel(\n        m,\n        {:?},\n        &[\n{}\n        ],\n    );", t.name, labels.join("\n"));
        }
        let mut lines = Vec::new();
        for f in al.fields.iter().filter(|f| names.contains(&f.name) && t.field(&f.name).is_none()) {
            let calc = f.calc_formula.as_deref().and_then(al::calc_formula).and_then(|cf| calc_expr(&cf, &al, &t.name, meta, src, &has));
            let flow = calc.is_some();
            if let Some(e) = field_expr(f, meta, &src.enums, calc) {
                lines.push(format!("            {e},"));
                if flow {
                    gen_flows.entry(t.name.clone()).or_default().push(f.name.clone());
                }
                known.entry(t.name.clone()).or_default().insert(f.name.to_ascii_lowercase());
            }
        }
        if !lines.is_empty() {
            let _ = writeln!(out, "    fields(\n        m,\n        {:?},\n        vec![\n{}\n        ],\n    );", t.name, lines.join("\n"));
        }
    }
    for (id, al) in &pages {
        let Some(p) = meta.pages.get(id) else { continue };
        let Some(names) = p.source_table.as_deref().and_then(|t| meta.table(t)).and_then(|t| known.get(&t.name)) else { continue };
        let has = |f: &str| names.contains(&f.to_ascii_lowercase());
        // The table's spelling of each field name.
        let spell = |f: &str| {
            meta.table(p.source_table.as_deref().unwrap_or_default())
                .and_then(|t| t.field(f).map(|d| d.name.clone()))
                .unwrap_or_else(|| f.to_string())
        };
        match p.kind {
            PageKind::List | PageKind::ListPart => {
                let cols: Vec<String> =
                    al.columns.iter().filter(|f| f.shown()).filter_map(|f| f.source.as_deref()).filter(|f| has(f)).map(spell).collect();
                if !cols.is_empty() {
                    let _ = writeln!(out, "    columns(m, {id:?}, {});", strs(&cols));
                }
            }
            PageKind::Card | PageKind::Document => {
                let mut tabs = Vec::new();
                for g in al.groups.iter().filter(|g| g.vis != al::Vis::Hidden) {
                    // One control per field: BC pairs controls behind opposite conditions (X / not X).
                    let mut seen_src = BTreeSet::new();
                    let shown: Vec<&al::AlField> = g
                        .fields
                        .iter()
                        .filter(|f| f.shown() && f.source.as_deref().is_some_and(has))
                        .filter(|f| f.source.as_deref().is_some_and(|s| seen_src.insert(s.to_ascii_lowercase())))
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    let pick = |keep: &dyn Fn(&al::AlField) -> bool| -> Vec<String> {
                        shown.iter().filter(|f| keep(f)).filter_map(|f| f.source.as_deref()).map(spell).collect()
                    };
                    let table_caps = p.source_table.as_deref().and_then(|t| meta.table(t)).and_then(|t| captions.get(&t.name));
                    let overrides: Vec<(String, String)> = shown
                        .iter()
                        .filter_map(|f| {
                            let (src, cap) = (f.source.as_deref()?, f.caption.as_deref()?);
                            let table_cap = table_caps.and_then(|c| c.get(&src.to_ascii_lowercase()));
                            (table_cap.map(String::as_str) != Some(cap)).then(|| (spell(src), cap.to_string()))
                        })
                        .collect();
                    // A heading where a captioned group first shows a field; a column break where a
                    // first-level group starts.
                    let mut headings = Vec::new();
                    let mut breaks = Vec::new();
                    let mut seen = BTreeSet::new();
                    let mut prev_column = None;
                    for f in &shown {
                        let Some(src) = f.source.as_deref() else { continue };
                        if let Some((id, Some(cap))) = &f.subgroup
                            && seen.insert(*id)
                        {
                            headings.push((spell(src), cap.clone()));
                        }
                        if f.column.is_some() && f.column != prev_column {
                            breaks.push(spell(src));
                        }
                        prev_column = f.column;
                    }
                    let mut t = format!("            Tab {{\n                caption: {:?},\n                fields: {},\n", g.caption, strs(&pick(&|_| true)));
                    for (label, v) in [("promoted", pick(&|f| f.promoted())), ("additional", pick(&|f| f.additional())), ("breaks", breaks)] {
                        if !v.is_empty() {
                            let _ = writeln!(t, "                {label}: {},", strs(&v));
                        }
                    }
                    for (label, v) in [("captions", overrides), ("headings", headings)] {
                        if !v.is_empty() {
                            let pairs: Vec<String> = v.iter().map(|(a, b)| format!("({a:?}, {b:?})")).collect();
                            let _ = writeln!(t, "                {label}: &[{}],", pairs.join(", "));
                        }
                    }
                    t.push_str("                ..Tab::default()\n            },");
                    tabs.push(t);
                }
                if !tabs.is_empty() {
                    let _ = writeln!(out, "    layout(\n        m,\n        {id:?},\n        &[\n{}\n        ],\n    );", tabs.join("\n"));
                }
            }
        }
    }
    // Navigation actions to pages we have, then list pages take their card's actions that BC also
    // offers on the list (they act on the selected row), then the action bar's categories.
    let norm = |s: &str| s.replace('&', "").trim_end_matches("...").trim().to_ascii_lowercase();
    let mut have_by_page: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let sheets: Vec<String> = pages.iter().filter(|(_, al)| al.kind.eq_ignore_ascii_case("Worksheet")).map(|(id, _)| (*id).to_string()).collect();
    if !sheets.is_empty() {
        let _ = writeln!(out, "    worksheets(m, {});", strs(&sheets));
    }
    for (id, al) in &pages {
        let Some(p) = meta.pages.get(id) else { continue };
        let own = p.source_table.as_deref().and_then(|t| meta.table(t));
        let mut have: BTreeSet<String> = p.actions.iter().map(|a| norm(&a.caption)).collect();
        let mut navs = Vec::new();
        for a in al.actions.iter().filter(|a| a.vis != al::Vis::Hidden) {
            let Some(target) = a.run_page.as_ref().and_then(|n| src.page_ids.get(n)).and_then(|oid| meta.pages.values().find(|q| q.object_id == *oid)) else { continue };
            let target_table = target.source_table.as_deref().and_then(|t| meta.table(t));
            let resolves = a.link.iter().all(|(t, s)| target_table.is_some_and(|tt| tt.field(t).is_some()) && own.is_some_and(|o| o.field(s).is_some()));
            if !resolves || !have.insert(norm(&a.caption)) {
                continue;
            }
            let link: Vec<String> = a.link.iter().map(|(t, s)| format!("({t:?}, {s:?})")).collect();
            navs.push(format!(
                "            Nav {{ caption: {:?}, page: {:?}, link: &[{}], shortcut: {:?} }},",
                a.caption,
                target.id,
                link.join(", "),
                a.shortcut.clone().unwrap_or_default()
            ));
        }
        if !navs.is_empty() {
            let _ = writeln!(out, "    actions(
        m,
        {id:?},
        &[
{}
        ],
    );", navs.join("
"));
        }
        have_by_page.insert(id.clone(), have);
    }
    for (id, al) in &pages {
        let Some(p) = meta.pages.get(id) else { continue };
        let Some(card) = p.card_page.as_ref() else { continue };
        let card_has = have_by_page.get(card).cloned().unwrap_or_default();
        let list_has = have_by_page.entry(id.clone()).or_default();
        let mirrored: Vec<String> = al
            .actions
            .iter()
            .filter(|a| a.vis != al::Vis::Hidden && card_has.contains(&norm(&a.caption)))
            .filter(|a| list_has.insert(norm(&a.caption)))
            .map(|a| a.caption.clone())
            .collect();
        if !mirrored.is_empty() {
            let _ = writeln!(out, "    mirror(m, {id:?}, {card:?}, {});", strs(&mirrored));
        }
    }
    for (id, al) in &pages {
        let have = have_by_page.get(id).cloned().unwrap_or_default();
        let caption_of = |name: &str| al.actions.iter().find(|a| a.name.eq_ignore_ascii_case(name) && a.vis != al::Vis::Hidden).map(|a| a.caption.clone());
        // Promoted sub-groups: action name -> "split » Posting » " / "menu » Calculate » ".
        let mut grouped: BTreeMap<String, String> = BTreeMap::new();
        for (cat, names) in al.categories.iter().filter(|(c, _)| c.starts_with('\u{a7}')) {
            let mut parts = cat.trim_start_matches('\u{a7}').splitn(2, '\u{a7}');
            let (kind, caption) = (parts.next().unwrap_or("menu"), parts.next().unwrap_or(""));
            for n in names {
                grouped.entry(n.to_ascii_lowercase()).or_insert_with(|| format!("{kind} \u{bb} {caption} \u{bb} "));
            }
        }
        // A part without promoted categories: its top-level action groups are the tabs.
        if al.categories.is_empty() && al.kind.eq_ignore_ascii_case("ListPart") && !al.action_groups.is_empty() {
            let tabs: Vec<String> = al
                .action_groups
                .iter()
                .filter_map(|(tab, members)| {
                    let mut seen = BTreeSet::new();
                    let acts: Vec<String> = members
                        .iter()
                        .filter_map(|(sub, n)| caption_of(n).map(|c| (sub, c)))
                        .filter(|(_, c)| have.contains(&norm(c)) && seen.insert(norm(c)))
                        .map(|(sub, c)| match sub {
                            Some(s) => format!("menu \u{bb} {s} \u{bb} {c}"),
                            None => c,
                        })
                        .collect();
                    (!acts.is_empty()).then(|| format!("            ({tab:?}, {}),", strs(&acts)))
                })
                .collect();
            if !tabs.is_empty() {
                let _ = writeln!(out, "    categories(\n        m,\n        {id:?},\n        &[\n{}\n        ],\n    );", tabs.join("\n"));
            }
            continue;
        }
        let cats: Vec<String> = al
            .categories
            .iter()
            .filter(|(cat, _)| !cat.starts_with('\u{a7}'))
            .filter_map(|(cat, names)| {
                let mut seen = BTreeSet::new();
                let acts: Vec<String> = names
                    .iter()
                    .filter_map(|n| caption_of(n).map(|c| (n, c)))
                    .filter(|(_, c)| have.contains(&norm(c)) && seen.insert(norm(c)))
                    .map(|(n, c)| format!("{}{c}", grouped.get(&n.to_ascii_lowercase()).map_or("", String::as_str)))
                    .collect();
                (!acts.is_empty()).then(|| format!("            ({cat:?}, {}),", strs(&acts)))
            })
            .collect();
        if !cats.is_empty() {
            let _ = writeln!(out, "    categories(
        m,
        {id:?},
        &[
{}
        ],
    );", cats.join("
"));
        }
    }
    // FlowField drill-downs: CalcFormula's table, opened on its drill-down page with the formula's
    // filters. Filters on FlowFilter fields (Date Filter, …) are empty by default and are dropped,
    // as are filters on fields the target table here doesn't have.
    let mut tables: Vec<&erpcraft_engine::meta::TableDef> = meta.tables.values().filter(|t| t.object_id > 0 && t.object_id < 50000).collect();
    tables.sort_by_key(|t| t.name.clone());
    let al_cache: BTreeMap<String, Option<AlTable>> = BTreeMap::new();
    let mut al_cache = al_cache;
    for t in tables {
        let hand_flows: Vec<String> = t.fields.iter().filter(|f| f.is_flow()).map(|f| f.name.clone()).collect();
        let flows: Vec<String> = hand_flows.iter().cloned().chain(gen_flows.get(&t.name).cloned().unwrap_or_default()).collect();
        if flows.is_empty() {
            continue;
        }
        let Some(al) = src.table(t.object_id) else { continue };
        let mut drills = Vec::new();
        let mut formulas = Vec::new();
        for name in &flows {
            let Some(cf) = al.fields.iter().find(|a| a.name.eq_ignore_ascii_case(name)).and_then(|a| a.calc_formula.as_deref()).and_then(al::calc_formula) else {
                continue;
            };
            if hand_flows.contains(name)
                && let Some(c) = calc_expr(&cf, &al, &t.name, meta, src, &has)
            {
                formulas.push(format!("            ({name:?}, {c}),"));
            }
            let Some(target) = meta.table(&cf.table) else { continue };
            let target_al = al_cache.entry(target.name.clone()).or_insert_with(|| src.table(target.object_id));
            let page = target_al
                .as_ref()
                .and_then(|ta| ta.drilldown_page.as_ref())
                .and_then(|n| src.page_ids.get(n))
                .and_then(|oid| meta.pages.values().find(|p| p.object_id == *oid))
                .or_else(|| meta.pages.values().find(|p| p.kind == PageKind::List && p.source_table.as_deref() == Some(target.name.as_str())));
            let Some(page) = page else { continue };
            let mut link = Vec::new();
            let mut fixed = Vec::new();
            for (tf, s) in &cf.clauses {
                if !has(&target.name, tf) {
                    continue;
                }
                match s {
                    al::CalcSrc::Field(own) => {
                        let flow_filter = al.fields.iter().any(|a| a.name.eq_ignore_ascii_case(own) && a.class == FieldClass::FlowFilter);
                        if !flow_filter && has(&t.name, own) {
                            link.push(format!("({tf:?}, {own:?})"));
                        }
                    }
                    al::CalcSrc::Expr(e) => fixed.push(format!("({tf:?}, {e:?})")),
                }
            }
            drills.push(format!("            ({name:?}, {:?}, &[{}], &[{}]),", page.id, link.join(", "), fixed.join(", ")));
        }
        if !formulas.is_empty() {
            let _ = writeln!(out, "    calcs(\n        m,\n        {:?},\n        vec![\n{}\n        ],\n    );", t.name, formulas.join("\n"));
        }
        if !drills.is_empty() {
            let _ = writeln!(out, "    drilldowns(\n        m,\n        {:?},\n        &[\n{}\n        ],\n    );", t.name, drills.join("\n"));
        }
    }
    out.push_str("}\n");
    out
}
