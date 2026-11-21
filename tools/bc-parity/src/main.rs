//! Compares ERPCraft's pages with the Business Central AL sources (BCApps, W1 Base Application),
//! and generates the fields and layouts that close the gap.
//!
//! ```text
//! cargo run -p bc-parity -- [--root <W1 BaseApp dir>] [--page <id>] [--summary] [--check]
//! cargo run -p bc-parity -- gen [--root <dir>] [--out crates/engine/src/schema/bc_gen.rs]
//! ```
//!
//! A field counts as shown in BC unless it has `Visible = false` or only application areas the
//! Essential experience leaves out. "Needs table field" means our table lacks the field (or the
//! generator can't map its type yet). `--check` exits with 1 while a page misses a field its
//! table already has, shows a field BC hides, puts it in another FastTab or orders it differently.

mod al;
mod generate;

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use erpcraft_engine::meta::{PageDef, PageKind, TableDef};

use crate::al::{AlPage, Vis};

const DEFAULT_ROOT: &str = r"D:\Kerja\BCApps\src\Layers\W1\BaseApp";
const DEFAULT_OUT: &str = "crates/engine/src/schema/bc_gen.rs";

#[derive(Default)]
struct Gaps {
    /// Shown in BC, our table has the field, our page doesn't show it.
    missing: Vec<String>,
    /// Shown in BC, our table lacks the field.
    needs_table: Vec<String>,
    /// On our page but hidden or absent in BC.
    extra: Vec<String>,
    /// On our page in a different FastTab than BC.
    misplaced: Vec<String>,
    order: Vec<String>,
    tabs: Vec<String>,
    actions_missing: Vec<String>,
}

impl Gaps {
    fn blocking(&self) -> usize {
        self.missing.len() + self.extra.len() + self.misplaced.len() + self.order.len() + self.tabs.len()
    }
}

fn compare(ours: &PageDef, table: Option<&TableDef>, al: &AlPage) -> Gaps {
    let mut g = Gaps::default();
    let has = |f: &str| table.is_some_and(|t| t.field(f).is_some());
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let mut bc_shown: HashSet<String> = HashSet::new();
    if matches!(ours.kind, PageKind::List | PageKind::ListPart) {
        let al_cols: Vec<&str> = al.columns.iter().filter(|f| f.shown()).filter_map(|f| f.source.as_deref()).collect();
        for src in &al_cols {
            bc_shown.insert(src.to_ascii_lowercase());
            if !ours.columns.iter().any(|c| same(c, src)) {
                if has(src) { g.missing.push((*src).to_string()) } else { g.needs_table.push((*src).to_string()) }
            }
        }
        g.extra.extend(ours.columns.iter().filter(|c| !bc_shown.contains(&c.to_ascii_lowercase())).cloned());
        let al_order: Vec<String> = al_cols.iter().filter(|s| ours.columns.iter().any(|c| same(c, s))).map(|s| s.to_ascii_lowercase()).collect();
        let our_order: Vec<String> = ours.columns.iter().map(|c| c.to_ascii_lowercase()).filter(|c| bc_shown.contains(c)).collect();
        if al_order != our_order {
            g.order.push("columns".to_string());
        }
    } else {
        let al_tabs: Vec<&al::AlGroup> = al.groups.iter().filter(|t| t.vis != Vis::Hidden && t.fields.iter().any(|f| f.shown() && f.source.as_deref().is_some_and(has))).collect();
        let al_caps: Vec<String> = al_tabs.iter().map(|t| t.caption.to_ascii_lowercase()).collect();
        let our_caps: Vec<String> = ours.groups.iter().map(|t| t.caption.to_ascii_lowercase()).collect();
        if al_caps != our_caps {
            g.tabs.push(format!("BC {al_caps:?} / ours {our_caps:?}"));
        }
        // BC can show a field in more than one FastTab, so look in the matching tab first.
        let our_tab_of = |f: &str, cap: &str| {
            let tabs: Vec<String> = ours.groups.iter().filter(|t| t.fields.iter().any(|x| same(x, f))).map(|t| t.caption.to_ascii_lowercase()).collect();
            tabs.iter().find(|c| *c == cap).or(tabs.first()).cloned()
        };
        for t in al.groups.iter().filter(|t| t.vis != Vis::Hidden) {
            let cap = t.caption.to_ascii_lowercase();
            let mut al_order = Vec::new();
            for f in t.fields.iter().filter(|f| f.shown()) {
                let Some(src) = &f.source else { continue };
                bc_shown.insert(src.to_ascii_lowercase());
                match our_tab_of(src, &cap) {
                    // Paired controls (X / not X) show one field once.
                    Some(c) if c == cap => {
                        if !al_order.contains(&src.to_ascii_lowercase()) {
                            al_order.push(src.to_ascii_lowercase());
                        }
                    }
                    Some(c) => g.misplaced.push(format!("{src} (BC: {}, ours: {c})", t.caption)),
                    None if has(src) => g.missing.push(format!("{}: {src}", t.caption)),
                    None => g.needs_table.push(format!("{}: {src}", t.caption)),
                }
            }
            if let Some(ot) = ours.groups.iter().find(|o| o.caption.to_ascii_lowercase() == cap) {
                let our_order: Vec<String> = ot.fields.iter().map(|f| f.to_ascii_lowercase()).filter(|f| al_order.contains(f)).collect();
                if our_order != al_order {
                    g.order.push(t.caption.clone());
                }
            }
        }
        for t in &ours.groups {
            g.extra.extend(t.fields.iter().filter(|f| !bc_shown.contains(&f.to_ascii_lowercase())).map(|f| format!("{}: {f}", t.caption)));
        }
    }
    let ours_actions: HashSet<String> = ours.actions.iter().map(|a| norm_caption(&a.caption)).collect();
    g.actions_missing.extend(al.actions.iter().filter(|a| a.vis != Vis::Hidden && !ours_actions.contains(&norm_caption(&a.caption))).map(|a| a.caption.clone()));
    g
}

fn norm_caption(s: &str) -> String {
    s.replace('&', "").trim_end_matches("...").trim().to_ascii_lowercase()
}

fn list(label: &str, items: &[String]) {
    if !items.is_empty() {
        println!("- **{label}** ({}): {}", items.len(), items.join(", "));
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let root = value("--root").or_else(|| std::env::var("BCAPPS_W1").ok()).unwrap_or_else(|| DEFAULT_ROOT.to_string());
    let src = generate::Sources::load(Path::new(&root));
    if src.pages.is_empty() {
        eprintln!("No *.Page.al files under {root}. Pass --root <BCApps>/src/Layers/W1/BaseApp.");
        return ExitCode::from(2);
    }

    if args.first().is_some_and(|a| a == "gen") {
        let out = value("--out").unwrap_or_else(|| DEFAULT_OUT.to_string());
        let code = generate::generate(&erpcraft_engine::schema::hand_written(), &src);
        return match std::fs::write(&out, code) {
            Ok(()) => {
                println!("Wrote {out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Could not write {out}: {e}");
                ExitCode::from(2)
            }
        };
    }

    let meta = erpcraft_engine::schema::base_app();
    let only = value("--page");
    let mut totals = Gaps::default();
    let mut blocking = 0;
    let mut unmatched = Vec::new();
    let mut details = Vec::new();
    println!("| Page | BC | Missing | Needs table field | Extra | Misplaced | Order | Tabs | Actions missing |");
    println!("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    for p in meta.pages.values() {
        if only.as_ref().is_some_and(|o| *o != p.id) || p.object_id == 0 || p.object_id >= 50000 {
            continue;
        }
        let Some(al) = src.page(p.object_id) else {
            unmatched.push(format!("{} ({})", p.id, p.object_id));
            continue;
        };
        let g = compare(p, p.source_table.as_deref().and_then(|t| meta.table(t)), &al);
        blocking += g.blocking();
        println!(
            "| {} | {} {} | {} | {} | {} | {} | {} | {} | {} |",
            p.id, al.id, al.name, g.missing.len(), g.needs_table.len(), g.extra.len(), g.misplaced.len(), g.order.len(), g.tabs.len(), g.actions_missing.len()
        );
        totals.missing.extend(g.missing.iter().cloned());
        totals.needs_table.extend(g.needs_table.iter().cloned());
        totals.extra.extend(g.extra.iter().cloned());
        totals.actions_missing.extend(g.actions_missing.iter().cloned());
        details.push((p.id.clone(), al, g));
    }
    println!(
        "\n**Totals:** missing {}, needs table field {}, extra {}, actions missing {}, blocking {blocking}.",
        totals.missing.len(),
        totals.needs_table.len(),
        totals.extra.len(),
        totals.actions_missing.len()
    );
    if !unmatched.is_empty() {
        println!("\nNo AL source found for: {}", unmatched.join(", "));
    }
    if !flag("--summary") {
        for (id, al, g) in &details {
            println!("\n## {id} ({} {}, {})", al.id, al.name, al.kind);
            list("Missing", &g.missing);
            list("Needs table field", &g.needs_table);
            list("Extra", &g.extra);
            list("Misplaced", &g.misplaced);
            list("Order", &g.order);
            list("Tabs", &g.tabs);
            list("Actions missing", &g.actions_missing);
        }
    }
    if flag("--check") && blocking > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
