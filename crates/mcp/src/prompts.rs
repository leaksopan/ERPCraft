//! MCP prompts: short workflow templates that steer an agent through common ERPCraft tasks.

use serde_json::{Map, Value, json};

use crate::jsonrpc::RpcError;

struct Arg {
    name: &'static str,
    description: &'static str,
    required: bool,
}

struct Prompt {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    args: &'static [Arg],
    render: fn(&Map<String, Value>) -> String,
}

static PROMPTS: &[Prompt] = &[
    Prompt {
        name: "create_sales_order",
        title: "Create a sales order",
        description: "Create a sales order for a customer, check availability and amounts, and post it only when confirmed.",
        args: &[
            Arg { name: "customer", description: "Customer no. or (part of) the name", required: true },
            Arg { name: "items", description: "What to sell, e.g. '2 x 1100, 1 x brake pads'", required: true },
        ],
        render: create_sales_order,
    },
    Prompt {
        name: "month_end_review",
        title: "Month-end review",
        description: "Review the books at month end: trial balance, overdue receivables and payables, unposted documents.",
        args: &[Arg { name: "as_of", description: "As-of date YYYY-MM-DD (default: the work date)", required: false }],
        render: month_end_review,
    },
    Prompt {
        name: "explain_page",
        title: "Explain a page",
        description: "Explain what a page shows and what its actions do.",
        args: &[Arg { name: "page", description: "Page id (e.g. customer-card)", required: true }],
        render: explain_page,
    },
];

fn arg<'a>(args: &'a Map<String, Value>, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).map(str::trim).unwrap_or("")
}

fn create_sales_order(a: &Map<String, Value>) -> String {
    format!(
        "Create a sales order in ERPCraft for customer \"{}\" with these items: {}.\n\n\
         1. Resolve the customer: if it is not a customer no., call record_list with {{\"table\": \"Customer\", \"search\": \"...\"}}. \
         Make sure it is not Blocked.\n\
         2. Resolve each item with record_list {{\"table\": \"Item\", \"search\": \"...\"}} and check stock with item_availability {{\"no\": \"...\"}}.\n\
         3. Create the order with sales_document_new {{\"document_type\": \"Order\", \"customer_no\": \"...\", \"lines\": [{{\"no\": \"<item no.>\", \"quantity\": n}}]}}.\n\
         4. Show the totals from sales_document_statistics {{\"document_type\": \"Order\", \"no\": \"<order no.>\"}}.\n\
         5. Ask me before posting. Only after I confirm, call sales_document_post and report the posted invoice no.",
        arg(a, "customer"),
        arg(a, "items")
    )
}

fn month_end_review(a: &Map<String, Value>) -> String {
    let as_of = match arg(a, "as_of") {
        "" => "the current work date (see company_info)".to_string(),
        d => d.to_string(),
    };
    format!(
        "Do a month-end review of the ERPCraft company as of {as_of}.\n\n\
         1. report_trial_balance (pass \"date\" if given): is it balanced? Summarise the largest balances.\n\
         2. Overdue receivables: record_list {{\"table\": \"Cust. Ledger Entry\", \"filters\": {{\"Open\": \"true\", \"Due Date\": \"<YYYY-MM-DD\"}}}} (the review date); group by customer.\n\
         3. Overdue payables: the same on \"Vendor Ledger Entry\".\n\
         4. Unposted documents: record_list with page sales-order-list, sales-invoice-list, purchase-order-list and purchase-invoices.\n\
         5. Finish with a short list of follow-ups (collections, payments due, documents to post). Do not post or change anything."
    )
}

fn explain_page(a: &Map<String, Value>) -> String {
    format!(
        "Explain the ERPCraft page \"{}\" to an ERP user.\n\n\
         Read it with page_describe (or the resource erpcraft://page/{{id}}), and its source table with table_describe. \
         Describe what the page is for, its sections or columns, its subform parts and details pane, and what each action does \
         (actions name the command they run).",
        arg(a, "page")
    )
}

pub fn list() -> Value {
    let prompts: Vec<Value> = PROMPTS
        .iter()
        .map(|p| {
            let args: Vec<Value> = p.args.iter().map(|a| json!({"name": a.name, "description": a.description, "required": a.required})).collect();
            json!({"name": p.name, "title": p.title, "description": p.description, "arguments": args})
        })
        .collect();
    json!({"prompts": prompts})
}

/// `prompts/get`.
pub fn get(params: &Value) -> Result<Value, RpcError> {
    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| RpcError::invalid_params("missing prompt 'name'"))?;
    let p = PROMPTS.iter().find(|p| p.name == name).ok_or_else(|| RpcError::invalid_params(format!("Unknown prompt '{name}'")))?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err(RpcError::invalid_params("prompt 'arguments' must be an object")),
    };
    for a in p.args.iter().filter(|a| a.required) {
        if arg(&args, a.name).is_empty() {
            return Err(RpcError::invalid_params(format!("prompt '{name}' needs argument '{}'", a.name)));
        }
    }
    Ok(json!({"description": p.description, "messages": [{"role": "user", "content": {"type": "text", "text": (p.render)(&args)}}]}))
}
