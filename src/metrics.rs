/// A scalar metric extracted from analysis results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub kind: MetricKind,
    pub dimension: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Ratio,
    Count,
    Score,
}
