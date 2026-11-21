//! A reader for the parts of AL that describe pages, tables and enums. AL is structured by
//! braces and keeps code in begin/end, so a brace tree with each level's `Name = value`
//! statements is enough. Strings, quoted names, comments and preprocessor lines are skipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Application areas the Essential experience (what a new BC company runs) does not show.
const HIDDEN_AREAS: &[&str] = &["manufacturing", "planning", "warehouse", "service", "adcs", "advanced"];

/// Page variables that are false in a standard company (no approvals, Dataverse, Office add-in,
/// job queue or manual document numbers), so the controls they show stay hidden. Checked against
/// the live web client where possible (No., Quote No. and VAT Date don't show on a sales order).
const FALSE_BY_DEFAULT: &[&str] = &[
    "openapprovalentriesexistforcurruser",
    "openapprovalentriesexistcurruser",
    "openapprovalentriesexist",
    "showworkflowstatus",
    "isofficeaddin",
    "crmintegrationenabled",
    "crmiscoupledtorecord",
    "cdsintegrationenabled",
    "docnovisible",
    "showquoteno",
    "vatdateenabled",
    "jobqueuevisible",
    "jobqueuesused",
    "jobqueueused",
    "isshiptocountyvisible",
    "isselltocountyvisible",
    "isbilltocountyvisible",
    "isbuyfromcountyvisible",
    "ispaytocountyvisible",
    "iscountyvisible",
    "isjournaltemplnamevisible",
    "entitytextenabled",
    "paymentservicevisible",
];

#[derive(Debug, Default)]
pub struct Node {
    pub head: String,
    pub props: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    /// `field("No."; Rec."No.")` → ("field", `"No."; Rec."No."`).
    pub fn kind(&self) -> (String, &str) {
        let h = self.head.trim();
        match (h.find('('), h.rfind(')')) {
            (Some(a), Some(b)) if b > a => (h[..a].trim().to_ascii_lowercase(), &h[a + 1..b]),
            _ => (h.split_whitespace().next().unwrap_or("").to_ascii_lowercase(), ""),
        }
    }

    fn caption_or(&self, fallback: &str) -> String {
        self.prop("Caption").and_then(literal).unwrap_or_else(|| unquote(fallback))
    }
}

pub fn parse(src: &str) -> Node {
    // Preprocessor lines (#if, #pragma) go; both branches of an #if stay, which is harmless here.
    let src: String = src.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut pending = String::new();
    let mut depth = 0i32;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|n| *n != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            '\'' => {
                pending.push(c);
                while let Some(n) = chars.next() {
                    pending.push(n);
                    if n == '\'' {
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            pending.push('\'');
                        } else {
                            break;
                        }
                    }
                }
            }
            '"' => {
                pending.push(c);
                for n in chars.by_ref() {
                    pending.push(n);
                    if n == '"' {
                        break;
                    }
                }
            }
            '(' => {
                depth += 1;
                pending.push(c);
            }
            ')' => {
                depth -= 1;
                pending.push(c);
            }
            ';' if depth <= 0 => flush(&mut stack, &mut pending),
            '{' => {
                let head = pending.trim().to_string();
                pending.clear();
                depth = 0;
                stack.push(Node { head, ..Node::default() });
            }
            '}' => {
                flush(&mut stack, &mut pending);
                depth = 0;
                if stack.len() > 1
                    && let Some(n) = stack.pop()
                    && let Some(parent) = stack.last_mut()
                {
                    parent.kids.push(n);
                }
            }
            _ => pending.push(c),
        }
    }
    while stack.len() > 1 {
        if let Some(n) = stack.pop()
            && let Some(parent) = stack.last_mut()
        {
            parent.kids.push(n);
        }
    }
    stack.pop().unwrap_or_default()
}

/// A statement `Name = value` becomes a property of the current level; anything else is code.
fn flush(stack: &mut [Node], pending: &mut String) {
    let s = pending.trim();
    if let Some((k, v)) = s.split_once('=') {
        let k = k.trim();
        if !k.is_empty()
            && k.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !v.starts_with('=')
            && let Some(top) = stack.last_mut()
        {
            top.props.push((k.to_string(), v.trim().to_string()));
        }
    }
    pending.clear();
}

pub fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

/// The first '...' literal of a property value (`Caption = 'General', Comment = '…'`).
pub fn literal(v: &str) -> Option<String> {
    let start = v.find('\'')?;
    let mut out = String::new();
    let mut chars = v[start + 1..].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                out.push('\'');
            } else {
                return Some(out);
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Splits at top-level ';' or ',' outside quotes and parentheses.
pub fn split_all(args: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quote = None;
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, _) if c == sep && depth == 0 => {
                out.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(args[start..].trim());
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Vis {
    Shown,
    Conditional,
    Hidden,
}

pub fn visibility(n: &Node) -> Vis {
    if let Some(a) = n.prop("ApplicationArea") {
        let areas: Vec<String> =
            a.split(',').map(|x| x.trim().trim_start_matches('#').to_ascii_lowercase()).filter(|x| !x.is_empty()).collect();
        if !areas.is_empty() && areas.iter().all(|x| HIDDEN_AREAS.contains(&x.as_str())) {
            return Vis::Hidden;
        }
    }
    match n.prop("Visible").map(str::to_ascii_lowercase).as_deref() {
        None | Some("true") => Vis::Shown,
        Some("false") => Vis::Hidden,
        Some(v) if FALSE_BY_DEFAULT.contains(&v.trim()) => Vis::Hidden,
        Some(_) => Vis::Conditional,
    }
}

fn combine(a: Vis, b: Vis) -> Vis {
    match (a, b) {
        (Vis::Hidden, _) | (_, Vis::Hidden) => Vis::Hidden,
        (Vis::Conditional, _) | (_, Vis::Conditional) => Vis::Conditional,
        _ => Vis::Shown,
    }
}

#[derive(Debug)]
pub struct AlField {
    /// The table field it shows; None for page variables and expressions.
    pub source: Option<String>,
    pub vis: Vis,
    pub importance: String,
    /// The page's own caption, when it overrides the table's.
    pub caption: Option<String>,
    /// The innermost nested group it sits in (a running number, and the group's caption if any).
    pub subgroup: Option<(usize, Option<String>)>,
    /// The first-level nested group of its FastTab: BC flows these into columns.
    pub column: Option<usize>,
}

impl AlField {
    pub fn shown(&self) -> bool {
        self.vis != Vis::Hidden
    }
    pub fn additional(&self) -> bool {
        self.importance.eq_ignore_ascii_case("Additional")
    }
    pub fn promoted(&self) -> bool {
        self.importance.eq_ignore_ascii_case("Promoted")
    }
}

#[derive(Debug)]
pub struct AlGroup {
    pub caption: String,
    pub vis: Vis,
    pub fields: Vec<AlField>,
}

#[derive(Debug)]
pub struct AlAction {
    pub name: String,
    pub caption: String,
    pub vis: Vis,
    /// `RunObject = Page "…"`: the page it opens.
    pub run_page: Option<String>,
    /// `RunPageLink = "Customer No." = field("No.")`, as (target field, source field).
    pub link: Vec<(String, String)>,
    pub shortcut: Option<String>,
}

#[derive(Debug, Default)]
pub struct AlPage {
    pub id: u32,
    pub name: String,
    pub kind: String,
    pub groups: Vec<AlGroup>,
    pub columns: Vec<AlField>,
    pub parts: Vec<String>,
    pub actions: Vec<AlAction>,
    /// Actions promoted to the action bar (`actionref` in `area(Promoted)`, or `Promoted = true`).
    pub promoted_actions: Vec<String>,
    /// The action bar's categories: caption and the names of the actions it shows, in order.
    pub categories: Vec<(String, Vec<String>)>,
}

fn al_field(n: &Node, inherited: Vis, subgroup: Option<(usize, Option<String>)>, column: Option<usize>) -> AlField {
    let (_, args) = n.kind();
    let parts = split_all(args, ';');
    let expr = parts.get(1).copied().unwrap_or("").trim();
    // `Rec.Name` or `"No."` is a table field; a bare name is a page variable.
    let (rec, expr) = match expr.strip_prefix("Rec.") {
        Some(e) => (true, e),
        None => (false, expr),
    };
    let quoted = expr.len() > 1 && expr.starts_with('"') && expr.ends_with('"');
    let word = !expr.is_empty() && expr.chars().all(|c| c.is_alphanumeric() || c == '_');
    AlField {
        source: (quoted || (rec && word)).then(|| unquote(expr)),
        vis: combine(inherited, visibility(n)),
        importance: n.prop("Importance").unwrap_or("Standard").to_string(),
        caption: n.prop("Caption").and_then(literal),
        subgroup,
        column,
    }
}

/// Walks a FastTab: fields keep the nested group they are in, so headings and columns can follow it.
struct Walk<'a> {
    out: &'a mut Vec<AlField>,
    columns: &'a mut Vec<AlField>,
    parts: &'a mut Vec<String>,
    next_group: usize,
}

impl Walk<'_> {
    /// Fields of a group, nested groups included (a FastTab's sub-groups are columns of one tab).
    fn fields(&mut self, n: &Node, vis: Vis, subgroup: Option<(usize, Option<String>)>, column: Option<usize>) {
        for k in &n.kids {
            let (kind, args) = k.kind();
            match kind.as_str() {
                "field" => self.out.push(al_field(k, vis, subgroup.clone(), column)),
                "group" | "grid" | "fixed" => {
                    self.next_group += 1;
                    let caption = k.prop("Caption").and_then(literal).filter(|c| !c.is_empty());
                    let column = column.or(Some(self.next_group));
                    self.fields(k, combine(vis, visibility(k)), Some((self.next_group, caption)), column);
                }
                "repeater" => {
                    for f in &k.kids {
                        if f.kind().0 == "field" {
                            self.columns.push(al_field(f, vis, None, None));
                        }
                    }
                }
                "part" if visibility(k) != Vis::Hidden => {
                    if let Some(target) = split_all(args, ';').get(1) {
                        self.parts.push(unquote(target));
                    }
                }
                _ => {}
            }
        }
    }
}

/// `"Customer No." = field("No."), Type = const(Sale)` → [("Customer No.", "No.")]; constants and
/// filters are dropped (the page opens unfiltered on them).
fn page_link(v: &str) -> Vec<(String, String)> {
    split_all(v, ',')
        .into_iter()
        .filter_map(|part| {
            let (target, rhs) = part.split_once('=')?;
            let rhs = rhs.trim();
            let inner = rhs.strip_prefix("field(").or_else(|| rhs.strip_prefix("FIELD("))?.strip_suffix(')')?;
            Some((unquote(target), unquote(inner)))
        })
        .collect()
}

fn collect_actions(n: &Node, area: &str, out: &mut Vec<AlAction>, promoted: &mut Vec<String>, categories: &mut Vec<(String, Vec<String>)>) {
    for k in &n.kids {
        let (kind, args) = k.kind();
        match kind.as_str() {
            "action" | "customaction" | "fileuploadaction" => {
                let run_page = k.prop("RunObject").and_then(|v| {
                    let v = v.trim();
                    v.get(..4).filter(|p| p.eq_ignore_ascii_case("page")).map(|_| unquote(&v[4..]))
                });
                if k.prop("Promoted").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                    promoted.push(unquote(args));
                }
                out.push(AlAction {
                    name: unquote(args),
                    caption: k.caption_or(args),
                    vis: visibility(k),
                    run_page,
                    link: k.prop("RunPageLink").map(page_link).unwrap_or_default(),
                    shortcut: k.prop("ShortCutKey").and_then(literal),
                });
            }
            "actionref" => {
                if let Some(target) = split_all(args, ';').get(1) {
                    promoted.push(unquote(target));
                }
            }
            "group" if area == "promoted" => {
                let before = promoted.len();
                collect_actions(k, area, out, promoted, categories);
                let names = promoted.get(before..).map(<[String]>::to_vec).unwrap_or_default();
                // Nested groups (split buttons) belong to the outer category.
                if n.kind().0 == "area" {
                    categories.push((k.caption_or(args), names));
                }
            }
            "area" => collect_actions(k, &args.trim().to_ascii_lowercase(), out, promoted, categories),
            "group" if visibility(k) != Vis::Hidden => collect_actions(k, area, out, promoted, categories),
            _ => {}
        }
    }
}

fn object<'a>(root: &'a Node, keyword: &str) -> Option<&'a Node> {
    root.kids.iter().find(|k| k.head.trim_start().to_ascii_lowercase().starts_with(&format!("{keyword} ")))
}

/// `page 21 "Customer Card"` → (21, "Customer Card").
fn object_id(head: &str) -> Option<(u32, String)> {
    let mut words = head.split_whitespace();
    words.next();
    let id = words.next()?.parse().ok()?;
    Some((id, unquote(&words.collect::<Vec<_>>().join(" "))))
}

pub fn al_page(src: &str) -> Option<AlPage> {
    let root = parse(src);
    let page = object(&root, "page")?;
    let (id, name) = object_id(&page.head)?;
    let mut p = AlPage { id, name, kind: page.prop("PageType").unwrap_or("Card").to_string(), ..AlPage::default() };
    for k in &page.kids {
        match k.kind().0.as_str() {
            "layout" => {
                for area in k.kids.iter().filter(|a| a.kind().1.trim().eq_ignore_ascii_case("content")) {
                    for g in &area.kids {
                        let (kind, args) = g.kind();
                        match kind.as_str() {
                            "group" => {
                                let vis = visibility(g);
                                let mut fields = Vec::new();
                                Walk { out: &mut fields, columns: &mut p.columns, parts: &mut p.parts, next_group: 0 }.fields(g, vis, None, None);
                                p.groups.push(AlGroup { caption: g.caption_or(args), vis, fields });
                            }
                            "repeater" => {
                                for f in &g.kids {
                                    if f.kind().0 == "field" {
                                        p.columns.push(al_field(f, Vis::Shown, None, None));
                                    }
                                }
                            }
                            "part" if visibility(g) != Vis::Hidden => {
                                if let Some(target) = split_all(args, ';').get(1) {
                                    p.parts.push(unquote(target));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "actions" => collect_actions(k, "", &mut p.actions, &mut p.promoted_actions, &mut p.categories),
            _ => {}
        }
    }
    Some(p)
}

#[derive(Debug, PartialEq)]
pub enum FieldClass {
    Normal,
    FlowField,
    FlowFilter,
}

#[derive(Debug)]
pub struct AlTableField {
    pub name: String,
    /// `Code[20]`, `Enum "Customer Blocked"`, `Option`, …
    pub ty: String,
    pub caption: String,
    pub tooltip: String,
    pub class: FieldClass,
    pub editable: bool,
    pub obsolete: bool,
    /// `TableRelation = "Payment Terms"` (a plain table, no field, condition or filter).
    pub relation: Option<String>,
    pub option_captions: Vec<String>,
    pub calc_formula: Option<String>,
    pub auto_format: u8,
}

#[derive(Debug, Default)]
pub struct AlTable {
    pub id: u32,
    pub name: String,
    pub fields: Vec<AlTableField>,
    /// `DrillDownPageId` (or `LookupPageId`): the page a FlowField over this table drills into.
    pub drilldown_page: Option<String>,
}

/// Where a CalcFormula filter takes its value from.
#[derive(Debug, PartialEq)]
pub enum CalcSrc {
    /// `field("No.")`: a field of the record the FlowField is on.
    Field(String),
    /// `const(Sale)` / `filter(<>0)`: a fixed filter expression.
    Expr(String),
}

/// `sum("Cust. Ledger Entry"."Amount" where("Customer No." = field("No."), Open = const(true)))`.
#[derive(Debug, PartialEq)]
pub struct CalcFormula {
    /// `sum`, `count`, `exist`, `lookup`, `min`, `max`, `average`.
    pub func: String,
    pub table: String,
    /// The value field (`"Amount (LCY)"`); empty for count / exist.
    pub field: String,
    pub clauses: Vec<(String, CalcSrc)>,
}

pub fn calc_formula(v: &str) -> Option<CalcFormula> {
    let v = v.trim();
    let inner = v.get(v.find('(')? + 1..v.rfind(')')?)?.trim();
    let lower = inner.to_ascii_lowercase();
    let (target, clauses) = match lower.find(" where(") {
        Some(i) => (&inner[..i], inner.get(i + 7..inner.len().saturating_sub(1)).unwrap_or("")),
        None => (inner, ""),
    };
    let parts = split_all(target.trim(), '.');
    let table = parts.first().map(|t| unquote(t))?;
    let field = parts.get(1).map(|f| unquote(f)).unwrap_or_default();
    let func = v.get(..v.find('(')?)?.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    for c in split_all(clauses, ',').into_iter().filter(|c| !c.is_empty()) {
        let (lhs, rhs) = c.split_once('=')?;
        let rhs = rhs.trim();
        let (kind, arg) = (rhs.get(..rhs.find('(')?)?.trim().to_ascii_lowercase(), rhs.get(rhs.find('(')? + 1..rhs.rfind(')')?)?);
        let src = match kind.as_str() {
            "field" => CalcSrc::Field(unquote(arg)),
            "const" => CalcSrc::Expr(unquote(arg).trim_matches('\'').to_string()),
            // AL quotes option names in filters ("Initial Entry"|Application); ERPCraft's syntax doesn't.
            "filter" => CalcSrc::Expr(arg.trim().replace('"', "")),
            _ => continue,
        };
        out.push((unquote(lhs), src));
    }
    Some(CalcFormula { func, table, field, clauses: out })
}

pub fn al_table(src: &str) -> Option<AlTable> {
    let root = parse(src);
    let table = object(&root, "table")?;
    let (id, name) = object_id(&table.head)?;
    let page = |k: &str| table.prop(k).map(|v| unquote(v.trim().trim_start_matches("Page").trim_start_matches("page")));
    let mut t = AlTable { id, name, fields: Vec::new(), drilldown_page: page("DrillDownPageId").or_else(|| page("LookupPageId")) };
    for f in table.kids.iter().filter(|k| k.kind().0 == "fields").flat_map(|k| k.kids.iter()) {
        let (kind, args) = f.kind();
        if kind != "field" {
            continue;
        }
        let parts = split_all(args, ';');
        let (Some(_), Some(name), Some(ty)) = (parts.first().and_then(|s| s.parse::<u32>().ok()), parts.get(1), parts.get(2)) else {
            continue;
        };
        let class = match f.prop("FieldClass").map(str::to_ascii_lowercase).as_deref() {
            Some("flowfield") => FieldClass::FlowField,
            Some("flowfilter") => FieldClass::FlowFilter,
            _ => FieldClass::Normal,
        };
        let relation = f.prop("TableRelation").map(str::trim).filter(|v| {
            !v.contains('.') && !v.contains(" where") && !v.contains(" WHERE") && !v.to_ascii_lowercase().starts_with("if ")
        });
        let options = f.prop("OptionCaption").and_then(literal).or_else(|| f.prop("OptionMembers").map(str::to_string));
        t.fields.push(AlTableField {
            name: unquote(name),
            ty: ty.trim().to_string(),
            caption: f.caption_or(name),
            tooltip: f.prop("ToolTip").and_then(literal).unwrap_or_default(),
            class,
            editable: !f.prop("Editable").is_some_and(|v| v.eq_ignore_ascii_case("false")),
            obsolete: f.prop("ObsoleteState").is_some(),
            relation: relation.map(unquote),
            option_captions: options.map(|o| split_all(&o, ',').into_iter().map(|s| unquote(s).trim_matches('\'').to_string()).collect()).unwrap_or_default(),
            calc_formula: f.prop("CalcFormula").map(str::to_string),
            auto_format: f.prop("AutoFormatType").and_then(|v| v.trim().parse().ok()).filter(|n| *n <= 2).unwrap_or(0),
        });
    }
    Some(t)
}

/// Enum name → value captions, in order (the blank value as "", like the hand-written schema).
pub fn al_enum(src: &str) -> Option<(String, Vec<String>)> {
    let root = parse(src);
    let e = object(&root, "enum")?;
    let (_, name) = object_id(&e.head)?;
    let values = e.kids.iter().filter(|k| k.kind().0 == "value").map(|v| v.caption_or(split_all(v.kind().1, ';').get(1).copied().unwrap_or("")).trim().to_string()).collect();
    Some((name, values))
}

/// Object name → id, read from each file's object line.
pub fn names(index: &BTreeMap<u32, PathBuf>) -> BTreeMap<String, u32> {
    index
        .iter()
        .filter_map(|(id, path)| {
            let text = std::fs::read_to_string(path).ok()?;
            let line = text.lines().find(|l| l.split_whitespace().nth(1).and_then(|w| w.parse::<u32>().ok()) == Some(*id))?;
            object_id(line).map(|(_, name)| (name, *id))
        })
        .collect()
}

/// Object id → file for every `*.<suffix>.al` under `root` whose object line starts with `keyword`.
pub fn index(root: &Path, suffix: &str, keyword: &str) -> BTreeMap<u32, PathBuf> {
    let mut out = BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    let ending = format!(".{suffix}.al");
    let prefix = format!("{keyword} ");
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.to_string_lossy().ends_with(&ending)
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Some(line) = text.lines().find(|l| l.starts_with(&prefix))
                && let Some(id) = line.split_whitespace().nth(1).and_then(|w| w.parse::<u32>().ok())
            {
                out.insert(id, path);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = r#"
namespace X;
using Y;
/// doc
page 21 "Customer Card"
{
    Caption = 'Customer Card';
    PageType = Card;
    layout
    {
        area(content)
        {
            group(General)
            {
                Caption = 'General';
                field("No."; Rec."No.")
                {
                    ApplicationArea = All;
                    Visible = NoFieldVisible;
                    trigger OnAssistEdit()
                    begin
                        if Rec.AssistEdit(xRec) then CurrPage.Update(); // { not a brace
                    end;
                }
#pragma warning disable AA0100
                field("Name 2"; Rec."Name 2") { ApplicationArea = All; Visible = false; }
                field(BalanceAsVendor; BalanceAsVendor) { Caption = 'Balance; as vendor'; }
                group(Sub) { field(Mfg; Rec.Mfg) { ApplicationArea = Manufacturing; } field(City; Rec.City) { Importance = Additional; } }
            }
            part(Lines; "Sales Lines") { SubPageLink = "No." = field("No."); }
        }
    }
    actions
    {
        area(processing)
        {
            action("&Post") { Caption = 'P&ost'; ShortCutKey = 'F9'; }
            action(Hidden) { Visible = false; }
        }
        area(navigation)
        {
            action(Entries) { RunObject = Page "Customer Ledger Entries"; RunPageLink = "Customer No." = field("No."), Open = const(true); }
        }
        area(Promoted) { group(Category_Process) { actionref(Post_Promoted; "&Post") { } } }
    }
}
"#;

    #[test]
    fn reads_layout_and_actions() {
        let p = al_page(CARD).unwrap();
        assert_eq!((p.id, p.name.as_str(), p.kind.as_str()), (21, "Customer Card", "Card"));
        let g = &p.groups[0];
        assert_eq!(g.caption, "General");
        let names: Vec<_> = g.fields.iter().map(|f| (f.source.clone(), f.vis)).collect();
        assert_eq!(
            names,
            vec![
                (Some("No.".into()), Vis::Conditional),
                (Some("Name 2".into()), Vis::Hidden),
                (None, Vis::Shown),
                (Some("Mfg".into()), Vis::Hidden),
                (Some("City".into()), Vis::Shown),
            ]
        );
        assert!(g.fields[4].additional());
        assert_eq!(p.parts, vec!["Sales Lines"]);
        assert_eq!(p.actions.len(), 3);
        assert_eq!((p.actions[0].caption.as_str(), p.actions[0].shortcut.as_deref()), ("P&ost", Some("F9")));
        assert_eq!(p.actions[1].vis, Vis::Hidden);
        assert_eq!(p.actions[2].run_page.as_deref(), Some("Customer Ledger Entries"));
        assert_eq!(p.actions[2].link, vec![("Customer No.".to_string(), "No.".to_string())]);
        assert_eq!(p.promoted_actions, vec!["&Post"]);
        assert_eq!(p.categories, vec![("Category_Process".to_string(), vec!["&Post".to_string()])]);
    }

    #[test]
    fn reads_calc_formulas() {
        let c = calc_formula(r#"sum("Detailed Cust. Ledg. Entry"."Amount (LCY)" where("Customer No." = field("No."),
            "Currency Code" = field("Currency Filter"), "Excluded from calculation" = const(false), Amount = filter(<>0)))"#)
        .unwrap();
        assert_eq!((c.func.as_str(), c.table.as_str(), c.field.as_str()), ("sum", "Detailed Cust. Ledg. Entry", "Amount (LCY)"));
        assert_eq!(
            c.clauses,
            vec![
                ("Customer No.".into(), CalcSrc::Field("No.".into())),
                ("Currency Code".into(), CalcSrc::Field("Currency Filter".into())),
                ("Excluded from calculation".into(), CalcSrc::Expr("false".into())),
                ("Amount".into(), CalcSrc::Expr("<>0".into())),
            ]
        );
        assert_eq!(calc_formula(r#"exist("Item Substitution" where(Type = const(Item), "No." = field("No.")))"#).unwrap().table, "Item Substitution");
    }

    #[test]
    fn reads_tables_and_enums() {
        let t = al_table(
            r#"table 18 Customer {
                fields {
                    field(1; "No."; Code[20]) { Caption = 'No.'; }
                    field(39; Blocked; Enum "Customer Blocked") { Caption = 'Blocked'; ToolTip = 'Specifies it''s blocked.'; }
                    field(59; "Balance (LCY)"; Decimal) { FieldClass = FlowField; CalcFormula = sum("Detailed Cust. Ledg. Entry"."Amount (LCY)" where("Customer No." = field("No."))); Editable = false; }
                    field(21; "Customer Posting Group"; Code[20]) { TableRelation = "Customer Posting Group"; }
                    field(7; City; Text[30]) { TableRelation = "Post Code".City; ObsoleteState = Pending; }
                    field(9; "Partner Type"; Option) { OptionMembers = " ",Company,Person; OptionCaption = ' ,Company,Person'; }
                }
            }"#,
        )
        .unwrap();
        assert_eq!((t.id, t.name.as_str(), t.fields.len()), (18, "Customer", 6));
        assert_eq!(t.fields[1].ty, r#"Enum "Customer Blocked""#);
        assert_eq!(t.fields[1].tooltip, "Specifies it's blocked.");
        assert_eq!(t.fields[2].class, FieldClass::FlowField);
        assert!(!t.fields[2].editable);
        assert!(t.fields[2].calc_formula.as_deref().is_some_and(|c| c.starts_with("sum(")));
        assert_eq!(t.fields[3].relation.as_deref(), Some("Customer Posting Group"));
        assert_eq!((t.fields[4].relation.as_deref(), t.fields[4].obsolete), (None, true));
        assert_eq!(t.fields[5].option_captions, vec!["", "Company", "Person"]);
        let (name, values) = al_enum(r#"enum 39 "Customer Blocked" { Extensible = true; value(0; " ") { Caption = ' '; } value(1; Ship) { Caption = 'Ship'; } }"#).unwrap();
        assert_eq!((name.as_str(), values), ("Customer Blocked", vec!["".to_string(), "Ship".to_string()]));
    }
}
