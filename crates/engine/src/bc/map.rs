//! Field mapping between Business Central API v2.0 entities and ERPCraft tables.
//!
//! Everything read from the API is hostile input: values of the wrong JSON type are ignored, text
//! is cut to the target field's length (by characters, never inside a UTF-8 sequence) and option
//! values that ERPCraft doesn't know are left alone.

use erpcraft_meta::{FieldDef, FieldType, Record, TableDef, Value};
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value as Json};

use crate::schema::tables::{CUSTOMER, GL_ACCOUNT, ITEM, VENDOR};

/// The BC API entities the connector moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    Customers,
    Vendors,
    Items,
    Accounts,
}

/// (API property, ERPCraft field) pairs, besides `number` → `No.`.
const CUSTOMER_FIELDS: &[(&str, &str)] = &[
    ("displayName", "Name"),
    ("addressLine1", "Address"),
    ("addressLine2", "Address 2"),
    ("city", "City"),
    ("postalCode", "Post Code"),
    ("country", "Country/Region Code"),
    ("phoneNumber", "Phone No."),
    ("email", "E-Mail"),
    ("website", "Home Page"),
    ("taxRegistrationNumber", "VAT Registration No."),
    ("blocked", "Blocked"),
    ("creditLimit", "Credit Limit (LCY)"),
];
const VENDOR_FIELDS: &[(&str, &str)] = &[
    ("displayName", "Name"),
    ("addressLine1", "Address"),
    ("addressLine2", "Address 2"),
    ("city", "City"),
    ("postalCode", "Post Code"),
    ("country", "Country/Region Code"),
    ("phoneNumber", "Phone No."),
    ("email", "E-Mail"),
    ("website", "Home Page"),
    ("taxRegistrationNumber", "VAT Registration No."),
    ("blocked", "Blocked"),
];
const ITEM_FIELDS: &[(&str, &str)] = &[
    ("displayName", "Description"),
    ("type", "Type"),
    ("baseUnitOfMeasureCode", "Base Unit of Measure"),
    ("unitPrice", "Unit Price"),
    ("unitCost", "Unit Cost"),
    ("blocked", "Blocked"),
];
const ACCOUNT_FIELDS: &[(&str, &str)] = &[
    ("displayName", "Name"),
    ("category", "Account Category"),
    ("accountType", "Account Type"),
    ("directPosting", "Direct Posting"),
    ("blocked", "Blocked"),
];

impl Entity {
    pub fn parse(s: &str) -> Option<Entity> {
        match s.trim().to_ascii_lowercase().as_str() {
            "customers" | "customer" => Some(Entity::Customers),
            "vendors" | "vendor" => Some(Entity::Vendors),
            "items" | "item" => Some(Entity::Items),
            "accounts" | "account" | "g/l accounts" => Some(Entity::Accounts),
            _ => None,
        }
    }

    /// The entity set name in the API URL.
    pub fn api_name(self) -> &'static str {
        match self {
            Entity::Customers => "customers",
            Entity::Vendors => "vendors",
            Entity::Items => "items",
            Entity::Accounts => "accounts",
        }
    }

    pub fn table(self) -> &'static str {
        match self {
            Entity::Customers => CUSTOMER,
            Entity::Vendors => VENDOR,
            Entity::Items => ITEM,
            Entity::Accounts => GL_ACCOUNT,
        }
    }

    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Entity::Customers => CUSTOMER_FIELDS,
            Entity::Vendors => VENDOR_FIELDS,
            Entity::Items => ITEM_FIELDS,
            Entity::Accounts => ACCOUNT_FIELDS,
        }
    }

    /// Fields a new record copies from the first existing record of its table, so imported
    /// customers, vendors and items can be posted to.
    pub fn default_fields(self) -> &'static [&'static str] {
        match self {
            Entity::Customers => &["Gen. Bus. Posting Group", "VAT Bus. Posting Group", "Customer Posting Group"],
            Entity::Vendors => &["Gen. Bus. Posting Group", "VAT Bus. Posting Group", "Vendor Posting Group"],
            Entity::Items => &["Gen. Prod. Posting Group", "VAT Prod. Posting Group", "Inventory Posting Group"],
            Entity::Accounts => &[],
        }
    }

    pub fn exportable(self) -> bool {
        !matches!(self, Entity::Accounts)
    }
}

/// The first `max` characters of `s`.
pub fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Lower-case letters and digits only: `Non_Inventory`, `Non-Inventory` and `NonInventory` match.
fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// An API value converted to fit field `f`, or `None` when it doesn't (wrong type, unknown option).
pub fn to_field(f: &FieldDef, v: &Json) -> Option<Json> {
    match &f.ty {
        FieldType::Code(len) | FieldType::Text(len) => {
            let s = match v {
                Json::Null => String::new(),
                Json::String(s) => s.clone(),
                Json::Number(n) => n.to_string(),
                _ => return None,
            };
            let s = if matches!(f.ty, FieldType::Code(_)) { s.trim().to_uppercase() } else { s };
            Some(Json::String(truncate(&s, usize::from(*len))))
        }
        FieldType::Option(opts) => {
            let s = v.as_str()?;
            // The API writes a blank option as " " or "_x0020_".
            let blank = s.trim().is_empty() || s.trim() == "_x0020_";
            let found = if blank {
                opts.iter().find(|o| o.is_empty())
            } else {
                let n = norm(s);
                opts.iter().find(|o| !o.is_empty() && norm(o) == n)
            };
            found.map(|o| Json::String(o.clone()))
        }
        // File content isn't part of the API entities ERPCraft syncs.
        FieldType::Blob => None,
        FieldType::Decimal => match v {
            Json::Number(_) => Some(v.clone()),
            Json::String(s) if s.trim().parse::<f64>().is_ok() => Some(v.clone()),
            _ => None,
        },
        FieldType::Boolean => v.as_bool().map(Json::Bool),
        FieldType::Integer | FieldType::Date => None,
    }
}

/// The ERPCraft values of one API record (without `No.`), keyed by field name.
pub fn from_api(entity: Entity, t: &TableDef, row: &Json) -> Map<String, Json> {
    let mut out = Map::new();
    for (api, field) in entity.fields() {
        let (Some(v), Some(f)) = (row.get(*api), t.field(field)) else { continue };
        if let Some(j) = to_field(f, v) {
            out.insert(f.name.clone(), j);
        }
    }
    if entity == Entity::Accounts
        && let Some(cat) = out.get("Account Category").and_then(Json::as_str)
    {
        let ib = match cat {
            "Assets" | "Liabilities" | "Equity" => Some("Balance Sheet"),
            "Income" | "Cost of Goods Sold" | "Expense" => Some("Income Statement"),
            _ => None,
        };
        if let Some(ib) = ib {
            out.insert("Income/Balance".into(), Json::String(ib.into()));
        }
    }
    out
}

/// The API body for a record: `number` plus the mapped fields.
pub fn to_api(entity: Entity, t: &TableDef, r: &Record) -> Map<String, Json> {
    let mut out = Map::new();
    out.insert("number".into(), Json::String(r.str("No.").to_string()));
    for (api, field) in entity.fields() {
        let Some(f) = t.field(field) else { continue };
        let v = r.get(&f.name);
        let j = match &f.ty {
            FieldType::Option(_) => {
                let s = v.as_str();
                Json::String(if s.is_empty() { " ".to_string() } else { s.replace('-', "_") })
            }
            FieldType::Decimal => v.as_dec().to_f64().and_then(serde_json::Number::from_f64).map_or(Json::from(0), Json::Number),
            FieldType::Boolean => Json::Bool(v.as_bool()),
            FieldType::Code(_) | FieldType::Text(_) => Json::String(v.as_str().to_string()),
            FieldType::Integer | FieldType::Date | FieldType::Blob => continue,
        };
        out.insert((*api).to_string(), j);
    }
    out
}

/// Whether a local API value equals the one Business Central holds.
pub fn same(local: &Json, remote: Option<&Json>) -> bool {
    let Some(remote) = remote else { return false };
    match (local, remote) {
        (Json::Number(a), Json::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-9,
            _ => false,
        },
        (Json::String(a), Json::String(b)) => a.trim() == b.trim(),
        (Json::String(a), Json::Null) => a.trim().is_empty(),
        _ => local == remote,
    }
}

/// Whether `values` would change record `r` (each value read the way `Validate` would).
pub fn changes(t: &TableDef, r: &Record, values: &Map<String, Json>) -> bool {
    values.iter().any(|(k, v)| match t.field(k) {
        Some(f) => Value::from_json(&f.ty, v).map_or(true, |nv| &nv != r.get(&f.name)),
        None => true,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::schema::base_app;
    use serde_json::json;

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate("ééééé", 3), "ééé");
        assert_eq!(truncate("ab", 5), "ab");
    }

    #[test]
    fn maps_options_and_lengths() {
        let meta = base_app();
        let item = meta.table(ITEM).unwrap();
        let v = from_api(Entity::Items, item, &json!({"type": "Non_Inventory", "displayName": "x".repeat(300), "blocked": "nope"}));
        assert_eq!(v["Type"], "Non-Inventory");
        assert_eq!(v["Description"].as_str().unwrap().chars().count(), 100);
        assert!(!v.contains_key("Blocked"));
        let acc = meta.table(GL_ACCOUNT).unwrap();
        let v = from_api(Entity::Accounts, acc, &json!({"category": "CostofGoodsSold", "accountType": "End_Total"}));
        assert_eq!(v["Account Category"], "Cost of Goods Sold");
        assert_eq!(v["Income/Balance"], "Income Statement");
        assert_eq!(v["Account Type"], "End-Total");
        let cust = meta.table(CUSTOMER).unwrap();
        let v = from_api(Entity::Customers, cust, &json!({"blocked": " ", "country": "us"}));
        assert_eq!(v["Blocked"], "");
        assert_eq!(v["Country/Region Code"], "US");
    }
}
