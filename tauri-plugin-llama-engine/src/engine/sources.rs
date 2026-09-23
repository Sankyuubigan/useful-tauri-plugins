//! Реестр источников бинарей движка (multi-repo).
//!
//! SSOT — встроенный JSON `engine_sources.json` (рядом с `models_catalog.json`,
//! компилируется через `include_str!`). Каждый источник описывает GitHub-репо,
//! префикс ассетов, паттерн cudart-архива, prefer_stable и runtime-аргументы
//! запуска (KVarN для BeeLlama). Юзер в UI видит ТОЛЬКО выбор источника —
//! кванты/хвосты захардкожены в реестре и не редактируются.

use serde::{Deserialize, Serialize};

/// Runtime-аргументы запуска llama-server, специфичные для источника.
/// `null` = не передавать флаг (или использовать дефолт/чекбоксы `kv_quant_*`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SourceRuntime {
    #[serde(default)]
    pub cache_type_k: Option<String>,
    #[serde(default)]
    pub cache_type_v: Option<String>,
    /// `--kv-tail-tokens N` (kvarn-хвост); None — не передавать.
    #[serde(default)]
    pub kv_tail_tokens: Option<u32>,
}

/// Спецификация одного источника движка.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceSpec {
    /// Стабильный id (папка в `backends/<id>/`, ключ `engine_source`).
    pub id: String,
    /// Человекочитаемая подпись для UI.
    pub label: String,
    /// Подсказка/пояснение для UI (не содержит юзер-кейсов).
    pub note: String,
    /// `owner/repo` на GitHub.
    pub repo: String,
    /// Префикс имени движкового ассета: `llama-`, `beellama-`, …
    pub asset_prefix: String,
    /// Подстрока для поиска cudart-архива (напр. `cudart-llama-bin`, `cudart`).
    pub cudart_pattern: String,
    /// true → брать только stable-релизы (тег без `preview`, не prerelease).
    #[serde(default)]
    pub prefer_stable: bool,
    /// Источник по умолчанию (обычно ggml-org).
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub runtime: SourceRuntime,
}

/// Инфо источника для UI (без runtime-квантов — юзеру не показывать).
#[derive(Serialize, Clone)]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
    pub note: String,
    pub installed: bool,
    pub is_default: bool,
}

const EMBEDDED_SOURCES: &str = include_str!("../../engine_sources.json");

/// Полный реестр источников (порядок как в JSON).
pub fn all_sources() -> Vec<SourceSpec> {
    serde_json::from_str(EMBEDDED_SOURCES).unwrap_or_default()
}

/// Спецификация по id; None если неизвестен.
pub fn source_spec(id: &str) -> Option<SourceSpec> {
    all_sources().into_iter().find(|s| s.id == id)
}

/// Дефолтный id источника (`ggml-org`, если в реестре не помечен default).
pub fn default_source_id() -> String {
    all_sources()
        .iter()
        .find(|s| s.default)
        .map(|s| s.id.clone())
        .unwrap_or_else(|| "ggml-org".to_string())
}

/// Известен ли id источника.
pub fn is_known_source(id: &str) -> bool {
    source_spec(id).is_some()
}

/// Резолв источника из конфига: None/пусто/неизвестный → дефолт, известный → как есть.
pub fn resolve_source(pref: Option<&str>) -> String {
    match pref {
        Some(p) if !p.is_empty() && is_known_source(p) => p.to_string(),
        _ => default_source_id(),
    }
}

/// Список источников для UI (installed проставляет вызывающий).
pub fn available_sources(installed_for: &dyn Fn(&str) -> bool) -> Vec<SourceInfo> {
    all_sources()
        .into_iter()
        .map(|s| SourceInfo {
            installed: installed_for(&s.id),
            is_default: s.default,
            id: s.id,
            label: s.label,
            note: s.note,
        })
        .collect()
}

/// URL списка релизов источника.
pub fn releases_url(spec: &SourceSpec) -> String {
    format!(
        "https://api.github.com/repos/{}/releases?per_page=30",
        spec.repo
    )
}

/// URL релиза по тегу (fallback nightly-tag).
pub fn release_by_tag_url(spec: &SourceSpec, tag: &str) -> String {
    format!(
        "https://api.github.com/repos/{}/releases/tags/{}",
        spec.repo, tag
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_parses_and_has_two_sources() {
        let all = all_sources();
        assert!(all.len() >= 2, "ожидалось ≥2 источника, got {}", all.len());
        let ggml = source_spec("ggml-org").expect("ggml-org");
        let bee = source_spec("beellama").expect("beellama");
        assert_eq!(ggml.asset_prefix, "llama-");
        assert_eq!(bee.asset_prefix, "beellama-");
        assert!(bee.prefer_stable);
        assert!(!ggml.prefer_stable);
        assert!(ggml.default);
        assert_eq!(bee.runtime.cache_type_k.as_deref(), Some("kvarn5"));
        assert_eq!(bee.runtime.cache_type_v.as_deref(), Some("kvarn4"));
        assert_eq!(bee.runtime.kv_tail_tokens, Some(1024));
        assert!(ggml.runtime.cache_type_k.is_none());
    }

    #[test]
    fn resolve_source_defaults_and_respects_user() {
        assert_eq!(resolve_source(None), "ggml-org");
        assert_eq!(resolve_source(Some("")), "ggml-org");
        assert_eq!(resolve_source(Some("beellama")), "beellama");
        assert_eq!(resolve_source(Some("unknown")), "ggml-org");
        assert!(is_known_source("beellama"));
        assert!(!is_known_source("rotorquant"));
    }

    #[test]
    fn releases_url_points_at_source_repo() {
        let bee = source_spec("beellama").unwrap();
        assert!(releases_url(&bee).contains("Anbeeld/beellama.cpp"));
        let ggml = source_spec("ggml-org").unwrap();
        assert!(releases_url(&ggml).contains("ggml-org/llama.cpp"));
    }

    #[test]
    fn available_sources_lists_with_default_flag() {
        let infos = available_sources(&|_| false);
        assert!(infos.iter().any(|i| i.id == "ggml-org" && i.is_default));
        assert!(infos.iter().any(|i| i.id == "beellama" && !i.is_default));
        assert!(infos.iter().all(|i| !i.installed));
    }
}
