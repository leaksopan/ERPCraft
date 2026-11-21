//! The page stack: what is open, newest last. Lists and the role center are the base; cards and
//! documents open over them as a stack of page dialogs (CoreERP / BC behaviour).

use erpcraft_engine::meta::Key;

#[derive(Clone, Debug)]
pub enum View {
    RoleCenter,
    List(ListState),
    Card(CardState),
}

#[derive(Clone, Debug)]
pub struct ListState {
    pub page: String,
    pub filters: Vec<(String, String)>,
    pub search: String,
    pub search_open: bool,
    pub selected: Option<Key>,
    /// Sort column and direction (descending when true); the page's key order otherwise.
    pub sort: Option<(String, bool)>,
    pub filter_pane: bool,
    /// The column of the selected row that has focus (framed, like BC's current cell).
    pub cell: Option<usize>,
    /// Rows marked for an action on several records (Ctrl / Shift + click, or Select More).
    pub marked: Vec<Key>,
    /// BC's Select More: a click marks or unmarks a row instead of moving to it.
    pub select_more: bool,
}

/// Where a card was opened from, so ‹ › can step through the same records.
#[derive(Clone, Debug)]
pub struct RecordNav {
    pub list_page: String,
    pub filters: Vec<(String, String)>,
    pub search: String,
}

#[derive(Clone, Debug)]
pub struct CardState {
    pub page: String,
    pub table: String,
    pub key: Key,
    pub selected_line: Option<Key>,
    /// The line cell being edited (column index), like BC's current cell in an editable part.
    pub line_cell: Option<usize>,
    /// The cell of the first empty row being typed in: the line is created only when a value is
    /// entered there (BC doesn't insert a line for a click).
    pub new_line_cell: Option<usize>,
    /// Cards open in view mode; ✏, a click on a field or on a FastTab header switches to edit.
    pub editing: bool,
    /// Expanded: the page dialog fills the content area (40 px inset) instead of 1328 px.
    pub expanded: bool,
    pub nav: Option<RecordNav>,
}

impl View {
    pub fn list(page: &str, filters: Vec<(String, String)>) -> Self {
        let filter_pane = !filters.is_empty();
        View::List(ListState { page: page.to_string(), filters, search: String::new(), search_open: false, selected: None, sort: None, filter_pane, cell: None, marked: Vec::new(), select_more: false })
    }

    pub fn card(page: &str, key: Key) -> Self {
        let table = erpcraft_engine::schema::base_app().page(page).and_then(|p| p.source_table.clone()).unwrap_or_default();
        View::Card(CardState {
            page: page.to_string(),
            table,
            key,
            selected_line: None,
            line_cell: None,
            new_line_cell: None,
            editing: false,
            expanded: false,
            nav: None,
        })
    }
}
