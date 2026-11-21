//! Table and field definitions (AL `table` objects).

use crate::record::Record;
use crate::value::Value;

/// AL data types ERPCraft stores. Lengths are AL's (`Code[20]`, `Text[100]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldType {
    Code(u16),
    Text(u16),
    Decimal,
    Integer,
    Date,
    Boolean,
    /// AL `Option` / `Enum`: the captions, the first being the default.
    Option(Vec<String>),
}

impl FieldType {
    pub fn option(values: &[&str]) -> Self {
        FieldType::Option(values.iter().map(|s| (*s).to_string()).collect())
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, FieldType::Decimal | FieldType::Integer)
    }

    pub fn name(&self) -> String {
        match self {
            FieldType::Code(n) => format!("Code[{n}]"),
            FieldType::Text(n) => format!("Text[{n}]"),
            FieldType::Decimal => "Decimal".into(),
            FieldType::Integer => "Integer".into(),
            FieldType::Date => "Date".into(),
            FieldType::Boolean => "Boolean".into(),
            FieldType::Option(_) => "Option".into(),
        }
    }
}

/// Where clicking a FlowField's value goes (BC's drill-down): a list page filtered on the record
/// (`link`: target field, this record's field) and on fixed expressions (`fixed`: field, filter).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillDown {
    pub page: String,
    pub link: Vec<(String, String)>,
    pub fixed: Vec<(String, String)>,
}

/// What a CalcFormula computes over the rows it selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcFunc {
    Sum,
    Count,
    Exist,
    Lookup,
    Min,
    Max,
    Average,
}

/// AL `CalcFormula`: `sum("Cust. Ledger Entry".Amount where("Customer No." = field("No.")))`.
/// `link` is (row field, this record's field); `fixed` is (row field, filter expression).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalcFormula {
    pub func: CalcFunc,
    pub table: String,
    /// The summed / looked-up field; empty for Count and Exist.
    pub field: String,
    pub link: Vec<(String, String)>,
    pub fixed: Vec<(String, String)>,
}

impl CalcFormula {
    pub fn new(func: CalcFunc, table: &str, field: &str, link: &[(&str, &str)], fixed: &[(&str, &str)]) -> Self {
        let pairs = |s: &[(&str, &str)]| s.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect();
        Self { func, table: table.to_string(), field: field.to_string(), link: pairs(link), fixed: pairs(fixed) }
    }
}

/// AL `FieldClass`: a stored field, or one the engine calculates when a record is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldClass {
    Normal,
    FlowField,
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub name: String,
    pub caption: String,
    pub ty: FieldType,
    pub class: FieldClass,
    /// AL `TableRelation` to the primary key of another table (single-field keys).
    pub relation: Option<String>,
    pub editable: bool,
    /// AL `NotBlank`: the field must have a value when the record is inserted.
    pub not_blank: bool,
    pub tooltip: String,
    /// AL `AutoFormatType` of a Decimal: 1 = amount (2 decimals), 2 = unit amount (2 to 5),
    /// 0 = as many decimals as the value has (up to 5), like BC's default.
    pub auto_format: u8,
    pub drilldown: Option<DrillDown>,
    /// A FlowField's formula; the engine evaluates it unless hand-written code sets the field.
    pub calc: Option<CalcFormula>,
}

impl FieldDef {
    pub fn new(name: &str, ty: FieldType) -> Self {
        Self {
            name: name.to_string(),
            caption: name.to_string(),
            ty,
            class: FieldClass::Normal,
            relation: None,
            editable: true,
            not_blank: false,
            tooltip: String::new(),
            auto_format: 0,
            drilldown: None,
            calc: None,
        }
    }

    pub fn code(name: &str, len: u16) -> Self {
        Self::new(name, FieldType::Code(len))
    }
    pub fn text(name: &str, len: u16) -> Self {
        Self::new(name, FieldType::Text(len))
    }
    pub fn decimal(name: &str) -> Self {
        Self::new(name, FieldType::Decimal)
    }
    pub fn integer(name: &str) -> Self {
        Self::new(name, FieldType::Integer)
    }
    pub fn date(name: &str) -> Self {
        Self::new(name, FieldType::Date)
    }
    pub fn boolean(name: &str) -> Self {
        Self::new(name, FieldType::Boolean)
    }
    pub fn option(name: &str, values: &[&str]) -> Self {
        Self::new(name, FieldType::option(values))
    }

    pub fn caption(mut self, c: &str) -> Self {
        self.caption = c.to_string();
        self
    }
    pub fn relation(mut self, table: &str) -> Self {
        self.relation = Some(table.to_string());
        self
    }
    pub fn flow(mut self) -> Self {
        self.class = FieldClass::FlowField;
        self.editable = false;
        self
    }
    /// A FlowField computed by `formula`.
    pub fn calc(mut self, formula: CalcFormula) -> Self {
        self.class = FieldClass::FlowField;
        self.editable = false;
        self.calc = Some(formula);
        self
    }
    pub fn read_only(mut self) -> Self {
        self.editable = false;
        self
    }
    pub fn not_blank(mut self) -> Self {
        self.not_blank = true;
        self
    }
    pub fn auto_format(mut self, n: u8) -> Self {
        self.auto_format = n;
        self
    }
    pub fn tooltip(mut self, t: &str) -> Self {
        self.tooltip = t.to_string();
        self
    }

    pub fn is_flow(&self) -> bool {
        self.class == FieldClass::FlowField
    }
}

#[derive(Clone, Debug)]
pub struct TableDef {
    /// AL table name, e.g. `Sales Header`. Also the key clients use.
    pub name: String,
    /// The BC object id (18 = Customer, 36 = Sales Header), kept for mapping to BC APIs later.
    pub object_id: u32,
    pub caption: String,
    pub fields: Vec<FieldDef>,
    pub primary_key: Vec<String>,
    /// Field shown next to the key when a record is named (`10000 · Adatum`).
    pub description_field: Option<String>,
    /// Page used to look up a value of this table (AL `LookupPageId`).
    pub lookup_page: Option<String>,
    /// Page that shows one record (AL `DrillDownPageId` / card).
    pub card_page: Option<String>,
}

impl TableDef {
    pub fn new(name: &str, object_id: u32) -> Self {
        Self {
            name: name.to_string(),
            object_id,
            caption: name.to_string(),
            fields: Vec::new(),
            primary_key: Vec::new(),
            description_field: None,
            lookup_page: None,
            card_page: None,
        }
    }

    pub fn field_def(mut self, f: FieldDef) -> Self {
        self.fields.push(f);
        self
    }

    pub fn fields(mut self, fs: Vec<FieldDef>) -> Self {
        self.fields.extend(fs);
        self
    }

    pub fn key(mut self, fields: &[&str]) -> Self {
        self.primary_key = fields.iter().map(|s| (*s).to_string()).collect();
        self
    }

    pub fn describe_by(mut self, field: &str) -> Self {
        self.description_field = Some(field.to_string());
        self
    }

    pub fn pages(mut self, lookup: &str, card: Option<&str>) -> Self {
        self.lookup_page = Some(lookup.to_string());
        self.card_page = card.map(str::to_string);
        self
    }

    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == name).or_else(|| self.fields.iter().find(|f| f.name.eq_ignore_ascii_case(name)))
    }

    /// A record with every field at its initial value (AL `Init`).
    pub fn init_record(&self) -> Record {
        let mut r = Record::default();
        for f in &self.fields {
            r.set(&f.name, Value::default_for(&f.ty));
        }
        r
    }

    /// `true` when the primary key is one Code field (so other tables can relate to it).
    pub fn single_code_key(&self) -> Option<&str> {
        match self.primary_key.as_slice() {
            [k] => Some(k.as_str()),
            _ => None,
        }
    }
}
