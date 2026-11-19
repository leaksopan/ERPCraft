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
    /// Cards open in view mode; ✏, a click on a field or on a FastTab header switches to edit.
    pub editing: bool,
    /// Expanded: the page dialog fills the content area (40 px inset) instead of 1328 px.
    pub expanded: bool,
    pub nav: Option<RecordNav>,
}

impl View {
    pub fn list(page: &str, filters: Vec<(String, String)>) -> Self {
        View::List(ListState { page: page.to_string(), filters, search: String::new(), search_open: false, selected: None })
    }

    pub fn card(page: &str, key: Key) -> Self {
        let table = erpcraft_engine::schema::base_app().page(page).and_then(|p| p.source_table.clone()).unwrap_or_default();
        View::Card(CardState {
            page: page.to_string(),
            table,
            key,
            selected_line: None,
            editing: false,
            expanded: false,
            nav: None,
        })
    }
}
