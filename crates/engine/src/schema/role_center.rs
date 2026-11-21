//! Role centers. The Business Manager one mirrors BC's page 9022 (Business Manager Role Center):
//! embedding links, creation actions, New / Payments / Reports menus, and the navigation sections.

use erpcraft_meta::{CueDef, CueGroupDef, Metadata, NavGroup as G, NavLink as L, RoleCenterDef};

pub const BUSINESS_MANAGER: &str = "business-manager";

pub fn register(m: &mut Metadata) {
    m.add_role_center(RoleCenterDef {
        id: BUSINESS_MANAGER.to_string(),
        object_id: 9022,
        caption: "Business Manager".to_string(),
        embedding: vec![
            L::new("Customers", "customer-list"),
            L::new("Vendors", "vendor-list"),
            L::new("Items", "item-list"),
            L::new("Bank Accounts", "bank-account-list"),
            L::new("Chart of Accounts", "chart-of-accounts"),
        ],
        creation: vec![
            L::new("Sales Quote", "sales-quote"),
            L::new("Sales Order", "sales-order"),
            L::new("Sales Invoice", "sales-invoice"),
            L::new("Purchase Quote", "purchase-quote"),
            L::new("Purchase Order", "purchase-order"),
            L::new("Purchase Invoice", "purchase-invoice"),
        ],
        action_groups: vec![
            G::new("New", vec![L::new("Customer", "customer-card"), L::new("Vendor", "vendor-card"), L::new("Item", "item-card")]),
            G::new(
                "Payments",
                vec![
                    L::new("Customer Ledger Entries", "customer-ledger-entries"),
                    L::new("Vendor Ledger Entries", "vendor-ledger-entries"),
                    L::new("Bank Account Ledger Entries", "bank-account-ledger-entries"),
                ],
            ),
            G::new(
                "Reports",
                vec![
                    L::new("Trial Balance", "chart-of-accounts"),
                    L::new("Financial Reports", "financial-reports"),
                    L::new("General Ledger Entries", "general-ledger-entries"),
                    L::new("Customer Balances", "customer-list"),
                    L::new("Vendor Balances", "vendor-list"),
                ],
            ),
        ],
        sections: vec![
            G::new(
                "Finance",
                vec![
                    L::new("Chart of Accounts", "chart-of-accounts"),
                    L::new("Financial Reports", "financial-reports"),
                    L::new("G/L Budgets", "gl-budget-names"),
                    L::new("Accounting Periods", "accounting-periods"),
                    L::new("General Ledger Entries", "general-ledger-entries"),
                    L::new("Customer Ledger Entries", "customer-ledger-entries"),
                    L::new("Vendor Ledger Entries", "vendor-ledger-entries"),
                    L::new("Currencies", "currencies"),
                    L::new("VAT Posting Setup", "vat-posting-setup"),
                    L::new("VAT Entries", "vat-entries"),
                    L::new("VAT Statements", "vat-statement-names"),
                    L::new("General Posting Setup", "general-posting-setup"),
                ],
            ),
            G::new(
                "Cash Management",
                vec![
                    L::new("Bank Accounts", "bank-account-list"),
                    L::new("Bank Account Ledger Entries", "bank-account-ledger-entries"),
                    L::new("Bank Account Reconciliations", "bank-acc-reconciliation-list"),
                    L::new("Bank Account Statements", "bank-account-statement-list"),
                    L::new("Payment Terms", "payment-terms"),
                ],
            ),
            G::new(
                "Sales",
                vec![
                    L::new("Customers", "customer-list"),
                    L::new("Items", "item-list"),
                    L::new("Sales Quotes", "sales-quotes"),
                    L::new("Sales Orders", "sales-order-list"),
                    L::new("Sales Invoices", "sales-invoice-list"),
                    L::new("Sales Credit Memos", "sales-credit-memos"),
                    L::new("Posted Sales Invoices", "posted-sales-invoices"),
                    L::new("Posted Sales Credit Memos", "posted-sales-credit-memos"),
                    L::new("Reminders", "reminder-list"),
                    L::new("Issued Reminders", "issued-reminder-list"),
                    L::new("Finance Charge Memos", "finance-charge-memo-list"),
                    L::new("Issued Fin. Charge Memos", "issued-fin-charge-memo-list"),
                    L::new("Reminder Terms", "reminder-terms"),
                    L::new("Finance Charge Terms", "finance-charge-terms"),
                    L::new("Salespeople", "salespersons"),
                ],
            ),
            G::new(
                "Purchasing",
                vec![
                    L::new("Vendors", "vendor-list"),
                    L::new("Items", "item-list"),
                    L::new("Purchase Quotes", "purchase-quotes"),
                    L::new("Purchase Orders", "purchase-order-list"),
                    L::new("Purchase Invoices", "purchase-invoices"),
                    L::new("Purchase Credit Memos", "purchase-credit-memos"),
                    L::new("Posted Purchase Invoices", "posted-purchase-invoices"),
                    L::new("Posted Purchase Credit Memos", "posted-purchase-credit-memos"),
                ],
            ),
            G::new(
                "Inventory",
                vec![
                    L::new("Items", "item-list"),
                    L::new("Item Ledger Entries", "item-ledger-entries"),
                    L::new("Transfer Orders", "transfer-orders"),
                    L::new("Posted Transfer Shipments", "posted-transfer-shipments"),
                    L::new("Posted Transfer Receipts", "posted-transfer-receipts"),
                    L::new("Physical Inventory Orders", "physical-inventory-orders"),
                    L::new("Phys. Inventory Recordings", "phys-inventory-recording-list"),
                    L::new("Posted Phys. Invt. Orders", "posted-phys-invt-order-list"),
                    L::new("Phys. Invt. Counting Periods", "phys-invt-counting-periods"),
                    L::new("Item Categories", "item-categories"),
                    L::new("Item Attributes", "item-attributes"),
                    L::new("Item Tracking Codes", "item-tracking-codes"),
                    L::new("Item Charges", "item-charges"),
                    L::new("Requisition Worksheets", "req-worksheet"),
                    L::new("Units of Measure", "units-of-measure"),
                    L::new("Inventory Posting Setup", "inventory-posting-setup"),
                ],
            ),
            G::new(
                "Setup",
                vec![
                    L::new("Company Information", "company-information"),
                    L::new("General Ledger Setup", "general-ledger-setup"),
                    L::new("Sales & Receivables Setup", "sales-receivables-setup"),
                    L::new("Purchases & Payables Setup", "purchases-payables-setup"),
                    L::new("Inventory Setup", "inventory-setup"),
                    L::new("No. Series", "no-series"),
                    L::new("Customer Posting Groups", "customer-posting-groups"),
                    L::new("Vendor Posting Groups", "vendor-posting-groups"),
                    L::new("Gen. Business Posting Groups", "gen-bus-posting-groups"),
                    L::new("Gen. Product Posting Groups", "gen-prod-posting-groups"),
                    L::new("VAT Business Posting Groups", "vat-bus-posting-groups"),
                    L::new("VAT Product Posting Groups", "vat-prod-posting-groups"),
                    L::new("Inventory Posting Groups", "inventory-posting-groups"),
                    L::new("Bank Account Posting Groups", "bank-posting-groups"),
                    L::new("Users", "users"),
                    L::new("Permission Sets", "permission-sets"),
                    L::new("Permissions", "permissions"),
                    L::new("User Permission Sets", "user-permission-sets"),
                ],
            ),
        ],
        cue_groups: vec![
            CueGroupDef {
                caption: "Ongoing Sales".to_string(),
                cues: vec![
                    CueDef::count("Sales Quotes", "sales-quotes"),
                    CueDef::count("Sales Orders", "sales-order-list"),
                    CueDef::count("Sales Invoices", "sales-invoice-list"),
                ],
            },
            CueGroupDef {
                caption: "Ongoing Purchases".to_string(),
                cues: vec![
                    CueDef::count("Purchase Orders", "purchase-order-list"),
                    CueDef::count("Purchase Invoices", "purchase-invoices"),
                ],
            },
            CueGroupDef {
                caption: "Payments".to_string(),
                cues: vec![
                    CueDef {
                        caption: "Overdue Sales Invoice Amount".to_string(),
                        page: "customer-ledger-entries".to_string(),
                        measure: "overdue:Remaining Amount".to_string(),
                    },
                    CueDef {
                        caption: "Overdue Purch. Invoice Amount".to_string(),
                        page: "vendor-ledger-entries".to_string(),
                        measure: "overdue:Remaining Amount".to_string(),
                    },
                    CueDef::count("Posted Sales Invoices", "posted-sales-invoices"),
                ],
            },
            CueGroupDef {
                caption: "Inventory".to_string(),
                cues: vec![CueDef::count("Items", "item-list"), CueDef::sum("Units in Stock", "item-list", "Inventory")],
            },
        ],
    });
}
