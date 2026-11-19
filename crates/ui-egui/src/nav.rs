//! The page stack: what is open, newest last (BC opens pages over each other with a back arrow).

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

#[derive(Clone, Debug)]
pub struct CardState {
    pub page: String,
    pub table: String,
    pub key: Key,
    pub selected_line: Option<Key>,
}

impl View {
    pub fn list(page: &str, filters: Vec<(String, String)>) -> Self {
        View::List(ListState { page: page.to_string(), filters, search: String::new(), search_open: false, selected: None })
    }

    pub fn card(page: &str, key: Key) -> Self {
        let table = erpcraft_engine::schema::base_app()
            .page(page)
            .and_then(|p| p.source_table.clone())
            .unwrap_or_default();
        View::Card(CardState { page: page.to_string(), table, key, selected_line: None })
    }
}
