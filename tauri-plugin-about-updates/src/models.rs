use serde::Serialize;

/// Описание одного релиза из GitHub Releases (для истории/отката).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub pub_date: Option<String>,
    pub notes: Option<String>,
    pub download_url: String,
    pub is_current: bool,
}
