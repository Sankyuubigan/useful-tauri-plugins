//! Реестр источников бинарей движка sd.cpp.
//!
//! SSOT — встроенный JSON `image_sources.json` (компилируется через
//! `include_str!`). Сейчас один источник — `leejet/stable-diffusion.cpp`.
//! Структура с уровнем source (`backends/<source>/<variant>/`) оставлена как
//! у llama-engine — под будущие форки.

use serde::{Deserialize, Serialize};

/// Спецификация одного источника движка.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceSpec {
    /// Стабильный id (папка в `backends/<id>/`).
    pub id: String,
    /// Человекочитаемая подпись для UI.
    pub label: String,
    /// Подсказка для UI.
    pub note: String,
    /// `owner/repo` на GitHub.
    pub repo: String,
    /// Префикс имени движкового ассета: `sd-`, …
    pub asset_prefix: String,
    /// Подстрока для поиска cudart-архива (напр. `cudart-sd-bin`).
    pub cudart_pattern: String,
    /// true → сначала искать среди stable-релизов, затем fallback на любой.
    #[serde(default)]
    pub prefer_stable: bool,
    /// Источник по умолчанию.
    #[serde(default)]
    pub default: bool,
}

/// Инфо источника для UI.
#[derive(Serialize, Clone)]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
    pub note: String,
    pub installed: bool,
    pub is_default: bool,
}

const EMBEDDED_SOURCES: &str = include_str!("../../image_sources.json");

/// Полный реестр источников (порядок как в JSON).
pub fn all_sources() -> Vec<SourceSpec> {
    serde_json::from_str(EMBEDDED_SOURCES).unwrap_or_default()
}

/// Спецификация по id; None если неизвестен.
pub fn source_spec(id: &str) -> Option<SourceSpec> {
    all_sources().into_iter().find(|s| s.id == id)
}

/// Дефолтный id источника.
pub fn default_source_id() -> String {
    all_sources()
        .iter()
        .find(|s| s.default)
        .map(|s| s.id.clone())
        .unwrap_or_else(|| "leejet".to_string())
}

/// Известен ли id источника.
pub fn is_known_source(id: &str) -> bool {
    source_spec(id).is_some()
}

/// URL списка релизов источника.
pub fn releases_url(spec: &SourceSpec) -> String {
    format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        spec.repo
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_parses_and_has_leejet() {
        let all = all_sources();
        assert!(!all.is_empty(), "реестр источников пуст");
        let leejet = source_spec("leejet").expect("leejet");
        assert_eq!(leejet.repo, "leejet/stable-diffusion.cpp");
        assert_eq!(leejet.asset_prefix, "sd-");
        assert!(leejet.default);
        assert!(is_known_source("leejet"));
        assert!(!is_known_source("unknown"));
        assert_eq!(default_source_id(), "leejet");
    }

    #[test]
    fn releases_url_points_at_repo() {
        let leejet = source_spec("leejet").unwrap();
        assert!(releases_url(&leejet).contains("leejet/stable-diffusion.cpp"));
    }
}
