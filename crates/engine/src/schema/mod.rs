//! The base application's metadata: tables, pages and role centers.

pub mod approvals;
pub mod attachments;
pub mod bank_rec;
pub mod catalog;
pub mod comments;
pub mod currency;
pub mod dimensions;
pub mod fin_reports;
pub mod finance;
pub mod fiscal;
pub mod fixed_assets;
pub mod inventory;
pub mod item_setup;
pub mod pages;
pub mod physinvt;
pub mod reminders;
pub mod role_center;
pub mod sku;
pub mod std_cost;
pub mod tables;
pub mod tracking;
pub mod transfer;
pub mod vat;

use std::sync::OnceLock;

use erpcraft_meta::Metadata;

/// The base application schema, built once from the hand-written clean-room definitions.
pub fn base_app() -> &'static Metadata {
    static META: OnceLock<Metadata> = OnceLock::new();
    META.get_or_init(hand_written)
}

/// Builds the complete clean-room metadata schema.
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
    inventory::register(&mut m);
    item_setup::register(&mut m);
    transfer::register(&mut m);
    tracking::register(&mut m);
    physinvt::register(&mut m);
    sku::register(&mut m);
    std_cost::register(&mut m);
    catalog::register(&mut m);
    fixed_assets::register(&mut m);
    dimensions::register(&mut m);
    comments::register(&mut m);
    approvals::register(&mut m);
    attachments::register(&mut m);
    crate::bc::register(&mut m);
    m
}
