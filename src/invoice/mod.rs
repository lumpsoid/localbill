pub mod mapper;
pub mod parser;

use serde::{Deserialize, Serialize};

/// One line-item returned by the Serbian fiscal API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceItem {
    pub name: String,
    pub quantity: f64,
    pub unit_price: f64,
    pub total: f64,
    #[serde(default)]
    pub gtin: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub label_rate: f64,
    #[serde(default)]
    pub tax_base_amount: f64,
    #[serde(default)]
    pub vat_amount: f64,
}

/// A fully-parsed invoice fetched from the Serbian fiscal authority website.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub invoice_number: String,
    pub retailer: String,
    /// ISO 8601 datetime, e.g. `2024-03-15T14:30:00`
    pub date: String,
    pub total_price: f64,
    pub currency: String,
    pub country: String,
    pub url: String,
    pub raw_bill_text: String,
    pub items: Vec<InvoiceItem>,
}

/// The data persisted as YAML front-matter for a single purchased item.
/// Intended for deserialising existing transaction files.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub date: String,
    pub retailer: String,
    pub name: String,
    pub quantity: f64,
    pub unit_price: f64,
    pub price_total: f64,
    pub currency: String,
    pub country: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
