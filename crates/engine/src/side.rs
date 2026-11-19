//! Sales and purchase documents are mirror images. [`Side`] names the fields that differ, so the
//! triggers, posting and commands are written once for both.

use crate::schema::tables::{
    CUST_LEDGER, CUSTOMER, PURCH_CR_HEADER, PURCH_CR_LINE, PURCH_HEADER, PURCH_INV_HEADER, PURCH_INV_LINE, PURCH_LINE,
    PURCH_SETUP, SALES_CR_HEADER, SALES_CR_LINE, SALES_HEADER, SALES_INV_HEADER, SALES_INV_LINE, SALES_LINE,
    SALES_SETUP, VEND_LEDGER, VENDOR,
};

pub struct Side {
    pub sales: bool,
    pub header: &'static str,
    pub line: &'static str,
    pub party_table: &'static str,
    pub party_caption: &'static str,
    /// Header: Sell-to Customer No. / Buy-from Vendor No.
    pub party_no: &'static str,
    pub party_name: &'static str,
    /// Header address prefix: "Sell-to " / "Buy-from ".
    pub addr: &'static str,
    /// Header: Bill-to Customer No. / Pay-to Vendor No.
    pub bill_no: &'static str,
    pub bill_name: &'static str,
    /// Customer / Vendor field naming its bill-to / pay-to party.
    pub party_bill_field: &'static str,
    pub posting_group: &'static str,
    pub person: &'static str,
    /// Line unit price field: Unit Price / Direct Unit Cost.
    pub price: &'static str,
    /// Line: Qty. to Ship / Qty. to Receive and Quantity Shipped / Quantity Received.
    pub qty_handle: &'static str,
    pub qty_handled: &'static str,
    pub setup: &'static str,
    pub ledger: &'static str,
    pub ledger_party: &'static str,
    pub inv_header: &'static str,
    pub inv_line: &'static str,
    pub cr_header: &'static str,
    pub cr_line: &'static str,
    pub command_prefix: &'static str,
}

pub const SALES: Side = Side {
    sales: true,
    header: SALES_HEADER,
    line: SALES_LINE,
    party_table: CUSTOMER,
    party_caption: "Customer",
    party_no: "Sell-to Customer No.",
    party_name: "Sell-to Customer Name",
    addr: "Sell-to ",
    bill_no: "Bill-to Customer No.",
    bill_name: "Bill-to Name",
    party_bill_field: "Bill-to Customer No.",
    posting_group: "Customer Posting Group",
    person: "Salesperson Code",
    price: "Unit Price",
    qty_handle: "Qty. to Ship",
    qty_handled: "Quantity Shipped",
    setup: SALES_SETUP,
    ledger: CUST_LEDGER,
    ledger_party: "Customer No.",
    inv_header: SALES_INV_HEADER,
    inv_line: SALES_INV_LINE,
    cr_header: SALES_CR_HEADER,
    cr_line: SALES_CR_LINE,
    command_prefix: "sales",
};

pub const PURCHASE: Side = Side {
    sales: false,
    header: PURCH_HEADER,
    line: PURCH_LINE,
    party_table: VENDOR,
    party_caption: "Vendor",
    party_no: "Buy-from Vendor No.",
    party_name: "Buy-from Vendor Name",
    addr: "Buy-from ",
    bill_no: "Pay-to Vendor No.",
    bill_name: "Pay-to Name",
    party_bill_field: "Pay-to Vendor No.",
    posting_group: "Vendor Posting Group",
    person: "Purchaser Code",
    price: "Direct Unit Cost",
    qty_handle: "Qty. to Receive",
    qty_handled: "Quantity Received",
    setup: PURCH_SETUP,
    ledger: VEND_LEDGER,
    ledger_party: "Vendor No.",
    inv_header: PURCH_INV_HEADER,
    inv_line: PURCH_INV_LINE,
    cr_header: PURCH_CR_HEADER,
    cr_line: PURCH_CR_LINE,
    command_prefix: "purchase",
};

impl Side {
    pub fn for_header(table: &str) -> Option<&'static Side> {
        match table {
            SALES_HEADER => Some(&SALES),
            PURCH_HEADER => Some(&PURCHASE),
            _ => None,
        }
    }

    pub fn for_line(table: &str) -> Option<&'static Side> {
        match table {
            SALES_LINE => Some(&SALES),
            PURCH_LINE => Some(&PURCHASE),
            _ => None,
        }
    }

    /// The setup field holding the number series for a document type.
    pub fn nos_field(&self, doc_type: &str) -> &'static str {
        match doc_type {
            "Quote" => "Quote Nos.",
            "Invoice" => "Invoice Nos.",
            "Credit Memo" => "Credit Memo Nos.",
            "Return Order" => "Return Order Nos.",
            _ => "Order Nos.",
        }
    }

    pub fn party_nos_field(&self) -> &'static str {
        if self.sales { "Customer Nos." } else { "Vendor Nos." }
    }

    /// The vendor's document number field that must be filled before posting.
    pub fn vendor_doc_no_field(doc_type: &str) -> &'static str {
        if matches!(doc_type, "Credit Memo" | "Return Order") { "Vendor Cr. Memo No." } else { "Vendor Invoice No." }
    }
}
