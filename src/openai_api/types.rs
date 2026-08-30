//! OpenAI Admin API types — the Costs API
//! (`GET /v1/organization/costs`) answers daily billed dollars grouped by
//! line item. The response is a paged list of `(day, line_item, amount)`
//! rows; this module only models the tolerant read (missing/null
//! `line_item`, unknown currencies) — aggregation lives in `fetch`.

use chrono::NaiveDate;
use serde::Deserialize;

/// One row of the costs list: a day's spend for one line item.
#[derive(Debug, Clone, Deserialize)]
pub struct CostRow {
    /// Unix seconds of the bucket start.
    pub start_time: i64,
    #[serde(default)]
    pub amount: Option<Amount>,
    /// Null when the request omitted `group_by` (rows lose their item then)
    /// or for fees without a model.
    #[serde(default)]
    pub line_item: Option<LineItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Amount {
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LineItem {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CostsPage {
    #[serde(default)]
    pub data: Vec<CostRow>,
    #[serde(default)]
    pub has_next_page: bool,
    #[serde(default)]
    pub next_page: Option<String>,
}

/// The date of a unix-seconds bucket start, in UTC.
pub fn bucket_date(start_time: i64) -> Option<NaiveDate> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(start_time, 0)
        .single()
        .map(|at| at.date_naive())
}
