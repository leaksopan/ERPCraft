//! Page and role center definitions (AL `page` objects and profiles).

/// AL `PageType`s ERPCraft renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    /// A list of records with an action bar, opening a card on drill-down.
    List,
    /// One record in FastTabs.
    Card,
    /// A header in FastTabs with a lines part (sales / purchase documents).
    Document,
    /// An editable grid embedded in a document (the lines subform).
    ListPart,
}

impl PageKind {
    pub fn name(self) -> &'static str {
        match self {
            PageKind::List => "List",
            PageKind::Card => "Card",
            PageKind::Document => "Document",
            PageKind::ListPart => "ListPart",
        }
    }
}

/// Where an action sits in the action bar (AL `area(...)` / action categories).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionArea {
    /// Shown on the bar itself (AL promoted actions / `actionref` in the Home category).
    Home,
    /// `Process` menu.
    Process,
    /// `New Document` / `Create` menu.
    New,
    /// `Related` menu (opens other pages).
    Navigate,
    /// `Report` menu.
    Report,
}

impl ActionArea {
    pub fn caption(self) -> &'static str {
        match self {
            ActionArea::Home => "Home",
            ActionArea::Process => "Process",
            ActionArea::New => "New Document",
            ActionArea::Navigate => "Related",
            ActionArea::Report => "Report",
        }
    }
}

/// One action. It either runs an engine command (with the current record's key as `key`) or opens
/// a page (filtered by `link`, current-record field → target field).
#[derive(Clone, Debug)]
pub struct ActionDef {
    pub caption: String,
    pub area: ActionArea,
    pub command: Option<String>,
    pub open_page: Option<String>,
    /// For `open_page`: (target field, current record field) filters.
    pub link: Vec<(String, String)>,
    pub shortcut: Option<String>,
    pub icon: String,
    pub tooltip: String,
    /// Ask before running (posting, deleting).
    pub confirm: Option<String>,
    /// Fixed command parameters (e.g. `document_type = Order` for "New Sales Order").
    pub params: Vec<(String, String)>,
    /// Parameters the user picks in a dialog first (BC's "Ship / Invoice / Ship and Invoice").
    pub ask: Vec<String>,
}

impl ActionDef {
    pub fn command(caption: &str, area: ActionArea, command: &str) -> Self {
        Self {
            caption: caption.to_string(),
            area,
            command: Some(command.to_string()),
            open_page: None,
            link: Vec::new(),
            shortcut: None,
            icon: String::new(),
            tooltip: String::new(),
            confirm: None,
            params: Vec::new(),
            ask: Vec::new(),
        }
    }

    pub fn page(caption: &str, area: ActionArea, page: &str) -> Self {
        Self {
            caption: caption.to_string(),
            area,
            command: None,
            open_page: Some(page.to_string()),
            link: Vec::new(),
            shortcut: None,
            icon: String::new(),
            tooltip: String::new(),
            confirm: None,
            params: Vec::new(),
            ask: Vec::new(),
        }
    }

    pub fn link(mut self, target_field: &str, source_field: &str) -> Self {
        self.link.push((target_field.to_string(), source_field.to_string()));
        self
    }
    pub fn shortcut(mut self, s: &str) -> Self {
        self.shortcut = Some(s.to_string());
        self
    }
    pub fn icon(mut self, s: &str) -> Self {
        self.icon = s.to_string();
        self
    }
    pub fn tooltip(mut self, s: &str) -> Self {
        self.tooltip = s.to_string();
        self
    }
    pub fn confirm(mut self, s: &str) -> Self {
        self.confirm = Some(s.to_string());
        self
    }
    pub fn ask(mut self, fields: &[&str]) -> Self {
        self.ask = fields.iter().map(|s| (*s).to_string()).collect();
        self
    }
    pub fn param(mut self, k: &str, v: &str) -> Self {
        self.params.push((k.to_string(), v.to_string()));
        self
    }
}

/// A FastTab on a card (AL `group`).
#[derive(Clone, Debug)]
pub struct GroupDef {
    pub caption: String,
    pub fields: Vec<String>,
    /// Fields shown on the collapsed FastTab header (AL `Importance = Promoted`).
    pub promoted: Vec<String>,
    pub collapsed: bool,
}

impl GroupDef {
    pub fn new(caption: &str, fields: &[&str]) -> Self {
        Self {
            caption: caption.to_string(),
            fields: fields.iter().map(|s| (*s).to_string()).collect(),
            promoted: Vec::new(),
            collapsed: false,
        }
    }

    pub fn promoted(mut self, fields: &[&str]) -> Self {
        self.promoted = fields.iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub fn collapsed(mut self) -> Self {
        self.collapsed = true;
        self
    }
}

/// An embedded page (AL `part`), linked to the host record.
#[derive(Clone, Debug)]
pub struct PartDef {
    pub caption: String,
    pub page: String,
    /// (part field, host field) pairs: AL `SubPageLink`.
    pub link: Vec<(String, String)>,
}

/// A FactBox in the right-hand pane: a few fields of the current record (or of the record a field
/// points at), shown read-only.
#[derive(Clone, Debug)]
pub struct FactBoxDef {
    pub caption: String,
    /// `None`: the page's own record. `Some(field)`: the record `field` relates to.
    pub via: Option<String>,
    pub fields: Vec<String>,
}

impl FactBoxDef {
    pub fn new(caption: &str, via: Option<&str>, fields: &[&str]) -> Self {
        Self {
            caption: caption.to_string(),
            via: via.map(str::to_string),
            fields: fields.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

/// A fixed filter on a page's records (AL `SourceTableView = where(...)`).
#[derive(Clone, Debug)]
pub struct ViewFilter {
    pub field: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct PageDef {
    /// Stable id clients use: `customer-list`, `sales-order`.
    pub id: String,
    /// The BC page object id (22 = Customer List), kept for mapping to BC later.
    pub object_id: u32,
    pub caption: String,
    pub kind: PageKind,
    pub source_table: Option<String>,
    /// List pages: the card or document page a row opens.
    pub card_page: Option<String>,
    /// Card / Document pages: the FastTabs.
    pub groups: Vec<GroupDef>,
    /// List / ListPart pages: the columns.
    pub columns: Vec<String>,
    pub parts: Vec<PartDef>,
    pub factboxes: Vec<FactBoxDef>,
    pub actions: Vec<ActionDef>,
    pub view: Vec<ViewFilter>,
    /// Field values a new record gets on this page (AL `SourceTableView` filters become defaults).
    pub editable: bool,
    /// Text searched by Tell Me in addition to the caption.
    pub keywords: String,
}

impl PageDef {
    pub fn new(id: &str, object_id: u32, caption: &str, kind: PageKind, table: Option<&str>) -> Self {
        Self {
            id: id.to_string(),
            object_id,
            caption: caption.to_string(),
            kind,
            source_table: table.map(str::to_string),
            card_page: None,
            groups: Vec::new(),
            columns: Vec::new(),
            parts: Vec::new(),
            factboxes: Vec::new(),
            actions: Vec::new(),
            view: Vec::new(),
            editable: true,
            keywords: String::new(),
        }
    }

    pub fn card(mut self, page: &str) -> Self {
        self.card_page = Some(page.to_string());
        self
    }
    pub fn columns(mut self, cols: &[&str]) -> Self {
        self.columns = cols.iter().map(|s| (*s).to_string()).collect();
        self
    }
    pub fn group(mut self, g: GroupDef) -> Self {
        self.groups.push(g);
        self
    }
    pub fn part(mut self, caption: &str, page: &str, link: &[(&str, &str)]) -> Self {
        self.parts.push(PartDef {
            caption: caption.to_string(),
            page: page.to_string(),
            link: link.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect(),
        });
        self
    }
    pub fn factbox(mut self, f: FactBoxDef) -> Self {
        self.factboxes.push(f);
        self
    }
    pub fn action(mut self, a: ActionDef) -> Self {
        self.actions.push(a);
        self
    }
    pub fn view(mut self, field: &str, value: &str) -> Self {
        self.view.push(ViewFilter { field: field.to_string(), value: value.to_string() });
        self
    }
    pub fn read_only(mut self) -> Self {
        self.editable = false;
        self
    }
    pub fn keywords(mut self, k: &str) -> Self {
        self.keywords = k.to_string();
        self
    }
}

/// A link in a role center's navigation: a caption and the page it opens, or the command it runs
/// (reports).
#[derive(Clone, Debug)]
pub struct NavLink {
    pub caption: String,
    pub page: String,
    pub command: Option<String>,
}

impl NavLink {
    pub fn new(caption: &str, page: &str) -> Self {
        Self { caption: caption.to_string(), page: page.to_string(), command: None }
    }

    pub fn command(caption: &str, command: &str) -> Self {
        Self { caption: caption.to_string(), page: String::new(), command: Some(command.to_string()) }
    }
}

/// A navigation menu (AL `area(sections)` group) or an action-bar menu.
#[derive(Clone, Debug)]
pub struct NavGroup {
    pub caption: String,
    pub links: Vec<NavLink>,
}

impl NavGroup {
    pub fn new(caption: &str, links: Vec<NavLink>) -> Self {
        Self { caption: caption.to_string(), links }
    }
}

/// A cue tile: the number of records matching filters, opening a list on click.
#[derive(Clone, Debug)]
pub struct CueDef {
    pub caption: String,
    pub page: String,
    /// `count` of the page's records, or `sum:<field>`.
    pub measure: String,
}

impl CueDef {
    pub fn count(caption: &str, page: &str) -> Self {
        Self { caption: caption.to_string(), page: page.to_string(), measure: "count".to_string() }
    }
    pub fn sum(caption: &str, page: &str, field: &str) -> Self {
        Self { caption: caption.to_string(), page: page.to_string(), measure: format!("sum:{field}") }
    }
}

#[derive(Clone, Debug)]
pub struct CueGroupDef {
    pub caption: String,
    pub cues: Vec<CueDef>,
}

/// The home page of a profile (AL `RoleCenter` page + `profile`).
#[derive(Clone, Debug)]
pub struct RoleCenterDef {
    pub id: String,
    pub object_id: u32,
    /// Profile name shown in settings, e.g. "Business Manager".
    pub caption: String,
    /// Top-level links next to the company name (AL `area(embedding)`).
    pub embedding: Vec<NavLink>,
    /// Navigation menus under the ☰ button (AL `area(sections)`).
    pub sections: Vec<NavGroup>,
    /// First actions on the bar (AL `area(creation)`).
    pub creation: Vec<NavLink>,
    /// Drop-down menus on the action bar (New ▾, Payments ▾, Reports ▾).
    pub action_groups: Vec<NavGroup>,
    pub cue_groups: Vec<CueGroupDef>,
}
