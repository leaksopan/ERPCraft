//! The base application's metadata: tables, pages and role centers.

pub mod finance;
pub mod fixed_assets;
pub mod inventory;
pub mod pages;
pub mod role_center;
pub mod tables;

use std::sync::OnceLock;

use erpcraft_meta::Metadata;

/// The base application, built once.
pub fn base_app() -> &'static Metadata {
    static META: OnceLock<Metadata> = OnceLock::new();
    META.get_or_init(|| {
        let mut m = Metadata::default();
        tables::register(&mut m);
        pages::register(&mut m);
        role_center::register(&mut m);
        finance::register(&mut m);
        inventory::register(&mut m);
        fixed_assets::register(&mut m);
        crate::bc::register(&mut m);
        m
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn metadata_is_consistent() {
        let problems = super::base_app().check();
        assert!(problems.is_empty(), "metadata problems:\n{}", problems.join("\n"));
    }
}
