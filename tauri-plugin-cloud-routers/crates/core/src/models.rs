//! Разбор идентификатора комбо и построение списка комбо для `/v1/models`.
//!
//! ## Форматы идентификаторов
//!
//! Per-provider комбо — строго `{provider_id}/{model_id}`, например:
//! `groq/llama-3.3-70b-versatile` или `together/meta-llama/Llama-3.3-70B-Instruct-Turbo`.
//!
//! Разделитель — **первый** `/`. Это важно: идентификаторы моделей у реальных
//! провайдеров сами содержат `/` (`meta-llama/…`, `Qwen/Qwen3-…`), а id
//! провайдера — никогда (это инвариант, проверяемый в [`crate::config::GatewayConfig::validate`]).
//! Поэтому однозначность не зависит от того, сколько слэшей в имени модели.
//!
//! Пользовательские комбо-связки ([`crate::config::ComboConfig`]) — просто
//! `name` **без** `/` (валидация конфига запрещает слэш в имени). Поэтому два
//! формата не пересекаются: id со слэшем разбирается как `провайдер/модель`,
//! без слэша — ищется среди пользовательских связок.
//!
//! ## Пользовательские связки
//!
//! У 9Router «комбо» — это настроенная пользователем цепочка моделей с
//! fallback. Здесь они тоже есть: имя + участники (провайдер, модель,
//! опциональное подмножество ключей), запрос идёт по цепочке до первого
//! успешного апстрима (см. `proxy::chat_completions`). Один id пользовательской
//! связки означает настроенную цепочку, а не один апстрим — в отличие от
//! per-provider комбо, которые остаются чистой функцией от (провайдер, модель).

use crate::config::{ComboConfig, DiscoveredModel, GatewayConfig, ProviderConfig};

/// Собрать id комбо из провайдера и модели.
pub fn combo_id(provider_id: &str, model_id: &str) -> String {
    format!("{}/{}", provider_id, model_id)
}

/// Разделить id комбо на (id провайдера, id модели) по первому `/`.
///
/// Возвращает `None`, если разделителя нет — тогда строка не является id комбо
/// этого шлюза, и запрос должен быть отклонён как «неизвестная модель», а не
/// молча отправлен первому попавшемуся провайдеру.
pub fn split_combo(combo: &str) -> Option<(&str, &str)> {
    let idx = combo.find('/')?;
    let provider = &combo[..idx];
    let model = &combo[idx + 1..];
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model))
}

/// Найти провайдера и апстрим-id модели по id комбо.
///
/// Провайдер должен быть **включён** и **маршрутизируем** (есть живой ключ и
/// обнаруженная модель). Проверка маршрутизируемости здесь, а не в прокси,
/// превращает «шлюз молча ушёл на провайдера без ключей» в понятную ошибку 404.
pub fn resolve_combo<'a>(
    providers: &'a [ProviderConfig],
    combo: &str,
) -> Option<(&'a ProviderConfig, &'a str)> {
    let (provider_id, model_id) = split_combo(combo)?;
    let provider = providers.iter().find(|p| p.id == provider_id)?;
    // Проверяем ТОЛЬКО «провайдер включён и модель обнаружена».
    //
    // Наличие живого ключа здесь НЕ проверяется намеренно. Ответ «модель не
    // найдена» (404) для модели, которая существует, но временно недоступна из-за
    // списанного ключа — ложь: она вводит в заблуждение и сам диагност, и IDE.
    // Отсутствие ключей — это отдельное состояние, и прокси сообщает о нём
    // честно: 503 с числом ключей (см. `proxy::chat_completions`).
    if !provider.is_enabled {
        return None;
    }
    // Модель обязана быть обнаружена: список моделей — единственный источник
    // правды о том, что провайдер вообще отдаёт. Ручной ввод id сюда не
    // подставляется.
    //
    // Возвращаем id **из конфига**, а не подстроку разобранного `combo`: так
    // lifetimes совпадают с провайдером, и вызывающий код получает ссылку,
    // живущую ровно столько же, сколько сама конфигурация. Строка равна
    // искомой по определению — это проверено сравнением выше.
    let model = provider.models.iter().find(|m| m.id == model_id)?;
    Some((provider, model.id.as_str()))
}

/// Одно комбо в формате `/api/combos` (контракт `client.rs` хоста).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComboInfo {
    /// Идентификатор комбо — именно его возвращает `/v1/models`.
    pub name: String,
    /// Тип комбо. Хост принимает `llm` либо отсутствие поля.
    pub kind: String,
    /// Реальные id моделей провайдера (для показа в UI).
    pub models: Vec<String>,
}

/// Все комбо маршрутизируемых провайдеров (per-provider).
///
/// Поле `models` содержит **только id этой модели** — раньше туда клонировался
/// весь каталог провайдера (N×M дублей в UI хоста; хост поле не читает,
/// проверено по `chat.ts`/`settings.ts`).
///
/// Порядок детерминирован (порядок провайдеров в конфиге, порядок моделей в
/// провайдере) — чтобы `/v1/models` не «дрожал» между двумя одинаковыми
/// запросами и чтобы UI не перерисовывался без причины.
pub fn list_combos(providers: &[ProviderConfig]) -> Vec<ComboInfo> {
    let mut out = Vec::new();
    for p in providers {
        if !p.is_routable() {
            continue;
        }
        for m in &p.models {
            if m.id.is_empty() {
                continue;
            }
            out.push(ComboInfo {
                name: combo_id(&p.id, &m.id),
                kind: "llm".to_string(),
                models: vec![m.id.clone()],
            });
        }
    }
    out
}

/// Все комбо шлюза — единственный построитель списка для `/v1/models`,
/// `/api/combos` и `read_combos_from_config` плагина (SSOT, «один построитель»).
///
/// Порядок: сначала пользовательские связки в порядке конфига, затем
/// per-provider комбо в порядке провайдеров. Пользовательские связки показываются
/// всегда (это явное намерение пользователя, хранящееся в конфиге), их
/// `models` — полные id участников `провайдер/модель` в порядке приоритета
/// fallback; доступность участников на момент показа не проверяется — честную
/// ошибку отдаст прокси при попытке маршрутизации.
pub fn list_all_combos(cfg: &GatewayConfig) -> Vec<ComboInfo> {
    let mut out = Vec::new();
    for c in &cfg.combos {
        out.push(ComboInfo {
            name: c.name.clone(),
            kind: "llm".to_string(),
            models: c.members.iter().map(|m| combo_id(&m.provider_id, &m.model_id)).collect(),
        });
    }
    out.extend(list_combos(&cfg.providers));
    out
}

/// Найти пользовательскую связку по имени — точное совпадение с `model=` запроса.
///
/// Имена связок без `/` (валидация конфига), поэтому совпадение возможно
/// только с полноценным именем, а не с префиксом per-provider id.
pub fn resolve_user_combo<'a>(cfg: &'a GatewayConfig, model: &str) -> Option<&'a ComboConfig> {
    cfg.combos.iter().find(|c| c.name == model)
}

/// Те же комбо в формате OpenAI `GET /v1/models`.
///
/// `owned_by` обязан быть ровно `"combo"`: хост (`client.rs`) отбрасывает всё
/// с другим значением, считая его локальной gguf-моделью. Это не украшение, а
/// часть контракта.
pub fn openai_models_list(cfg: &GatewayConfig, created: u64) -> serde_json::Value {
    let data: Vec<serde_json::Value> = list_all_combos(cfg)
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.name,
                "object": "model",
                "owned_by": "combo",
                "created": created,
            })
        })
        .collect();
    serde_json::json!({ "object": "list", "data": data })
}

/// Разбор ответа `GET {base_url}/models` провайдера в список моделей.
///
/// Терпимо к мусорному провайдеру: записи без строкового `id` пропускаются, дубли
/// схлопываются с сохранением порядка. Пустой результат — валидный, но
/// бессмысленный ответ: вызывающий код должен сообщить пользователю, что
/// провайдер вернул 0 моделей, а не молча записать пустой список.
pub fn parse_models_response(body: &str) -> Result<Vec<DiscoveredModel>, String> {
    let root: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("Ответ /models не является JSON: {}", e))?;
    let arr = root
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "В ответе /models нет массива \"data\"".to_string())?;
    let mut out: Vec<DiscoveredModel> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for item in arr {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || seen.iter().any(|s| s == id) {
            continue;
        }
        seen.push(id.to_string());
        out.push(DiscoveredModel {
            id: id.to_string(),
            object: item.get("object").and_then(|v| v.as_str()).map(|s| s.to_string()),
            created: item.get("created").and_then(|v| v.as_u64()),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccountKey;

    fn provider(id: &str, models: &[&str]) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            name: id.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            is_enabled: true,
            relay_id: None,
            keys: vec![AccountKey { id: "k1".into(), key: "sk-1".into(), is_active: true, name: None }],
            models: models.iter().map(|m| DiscoveredModel { id: m.to_string(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn split_combo_uses_first_slash_only() {
        // Модель сама содержит слэш — он обязан остаться в модели.
        assert_eq!(split_combo("groq/llama-3.3-70b"), Some(("groq", "llama-3.3-70b")));
        assert_eq!(
            split_combo("together/meta-llama/Llama-3.3-70B-Instruct"),
            Some(("together", "meta-llama/Llama-3.3-70B-Instruct"))
        );
    }

    #[test]
    fn split_combo_rejects_malformed_ids() {
        assert_eq!(split_combo("llama-3.3-70b"), None, "без разделителя");
        assert_eq!(split_combo("/model"), None, "пустой провайдер");
        assert_eq!(split_combo("groq/"), None, "пустая модель");
        assert_eq!(split_combo(""), None);
    }

    #[test]
    fn combo_id_is_inverse_of_split_combo() {
        for (p, m) in [("groq", "llama-3.3-70b"), ("together", "meta-llama/Llama-3.3-70B")] {
            let id = combo_id(p, m);
            assert_eq!(split_combo(&id), Some((p, m)), "{id}");
        }
    }

    #[test]
    fn resolve_combo_finds_provider_and_model() {
        let ps = vec![provider("groq", &["llama-3.3-70b-versatile"])];
        let (p, m) = resolve_combo(&ps, "groq/llama-3.3-70b-versatile").expect("resolve");
        assert_eq!(p.id, "groq");
        assert_eq!(m, "llama-3.3-70b-versatile");
    }

    #[test]
    fn resolve_combo_rejects_unknown_provider() {
        let ps = vec![provider("groq", &["m1"])];
        assert!(resolve_combo(&ps, "deepseek/m1").is_none());
    }

    #[test]
    fn resolve_combo_rejects_model_not_discovered() {
        let ps = vec![provider("groq", &["m1"])];
        assert!(
            resolve_combo(&ps, "groq/m2").is_none(),
            "нельзя слать на модель, которой нет в обнаруженных — список единственный источник правды"
        );
    }

    #[test]
    fn resolve_combo_rejects_disabled_provider() {
        let mut ps = vec![provider("groq", &["m1"])];
        ps[0].is_enabled = false;
        assert!(resolve_combo(&ps, "groq/m1").is_none());
    }

    #[test]
    fn resolve_combo_succeeds_even_if_all_keys_deactivated() {
        // Ключи могли быть выключены апстримом (401). Модель при этом остаётся
        // в конфиге, и 404 «нет такой модели» был бы неправдой. Отсутствие
        // живых ключей — это 503, и его решает прокси, зная числа.
        let mut ps = vec![provider("groq", &["m1"])];
        ps[0].keys[0].is_active = false;
        assert!(resolve_combo(&ps, "groq/m1").is_some());
    }

    #[test]
    fn resolve_combo_rejects_provider_without_models() {
        let mut ps = vec![provider("groq", &[])];
        assert!(resolve_combo(&ps, "groq/m1").is_none());
    }

    #[test]
    fn list_combos_skips_unroutable_providers() {
        let mut dead = provider("dead", &["m1"]);
        dead.is_enabled = false;
        let nokeys = provider("nokeys", &["m1"]);
        let mut nokeys = nokeys;
        nokeys.keys.clear();
        let ps = vec![dead, nokeys, provider("groq", &["m1", "m2"])];
        let combos = list_combos(&ps);
        assert_eq!(combos.len(), 2);
        assert_eq!(combos[0].name, "groq/m1");
        assert_eq!(combos[1].name, "groq/m2");
    }

    #[test]
    fn list_combos_reports_only_own_model() {
        // Регресс 27.10.2026: в models клонировался ВЕСЬ каталог провайдера на
        // каждую запись (N×M дублей в плашке хоста). Теперь — только эта модель.
        let ps = vec![provider("groq", &["m1", "m2"])];
        let combos = list_combos(&ps);
        assert_eq!(combos[0].name, "groq/m1");
        assert_eq!(combos[0].models, vec!["m1".to_string()], "только своя модель, не каталог");
        assert_eq!(combos[1].name, "groq/m2");
        assert_eq!(combos[1].models, vec!["m2".to_string()]);
    }

    #[test]
    fn list_all_combos_puts_user_combos_first_with_member_ids() {
        let ps = vec![provider("groq", &["m1"]), provider("deep", &["d1"])];
        let mut cfg = GatewayConfig::default();
        cfg.providers = ps.clone();
        cfg.combos.push(crate::config::ComboConfig {
            name: "fast".into(),
            members: vec![
                crate::config::ComboMember { provider_id: "deep".into(), model_id: "d1".into(), key_ids: None },
                crate::config::ComboMember { provider_id: "groq".into(), model_id: "m1".into(), key_ids: None },
            ],
        });
        let combos = list_all_combos(&cfg);
        assert_eq!(combos[0].name, "fast", "пользовательские связки первыми");
        assert_eq!(
            combos[0].models,
            vec!["deep/d1".to_string(), "groq/m1".to_string()],
            "участники в порядке приоритета fallback"
        );
        assert_eq!(combos[1].name, "groq/m1", "затем per-provider комбо");
        assert_eq!(combos[2].name, "deep/d1");
    }

    #[test]
    fn list_all_combos_shows_user_combo_even_if_provider_disabled() {
        // Связка — явное намерение пользователя в конфиге: её прятать нельзя,
        // иначе «куда делась моя связка». Недоступность участников честно
        // отдаст прокси при попытке маршрутизации.
        let mut ps = vec![provider("groq", &["m1"])];
        ps[0].is_enabled = false;
        let mut cfg = GatewayConfig::default();
        cfg.providers = ps;
        cfg.combos.push(crate::config::ComboConfig {
            name: "fast".into(),
            members: vec![crate::config::ComboMember {
                provider_id: "groq".into(),
                model_id: "m1".into(),
                key_ids: None,
            }],
        });
        let combos = list_all_combos(&cfg);
        assert_eq!(combos.len(), 1, "выключенный провайдер скрывает per-provider комбо, но не пользовательскую связку");
        assert_eq!(combos[0].name, "fast");
    }

    #[test]
    fn resolve_user_combo_finds_exact_name_only() {
        let mut cfg = GatewayConfig::default();
        cfg.combos.push(crate::config::ComboConfig {
            name: "fast".into(),
            members: vec![crate::config::ComboMember {
                provider_id: "groq".into(),
                model_id: "m1".into(),
                key_ids: None,
            }],
        });
        assert!(resolve_user_combo(&cfg, "fast").is_some());
        assert!(resolve_user_combo(&cfg, "fastest").is_none(), "префикс не совпадает");
        assert!(resolve_user_combo(&cfg, "groq/m1").is_none(), "per-provider id не имя связки");
    }

    #[test]
    fn openai_list_includes_user_combo_and_keeps_owned_by() {
        let ps = vec![provider("groq", &["m1"])];
        let mut cfg = GatewayConfig::default();
        cfg.providers = ps;
        cfg.combos.push(crate::config::ComboConfig {
            name: "fast".into(),
            members: vec![crate::config::ComboMember {
                provider_id: "groq".into(),
                model_id: "m1".into(),
                key_ids: None,
            }],
        });
        let v = openai_models_list(&cfg, 1_700_000_000);
        let data = v["data"].as_array().expect("data array");
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["id"], "fast");
        assert_eq!(data[0]["owned_by"], "combo");
        assert_eq!(data[1]["id"], "groq/m1");
    }

    #[test]
    fn openai_list_uses_owned_by_combo_exactly() {
        // Хост (client.rs) отбрасывает всё, у чего owned_by != "combo".
        let mut cfg = GatewayConfig::default();
        cfg.providers = vec![provider("groq", &["m1"])];
        let v = openai_models_list(&cfg, 1_700_000_000);
        assert_eq!(v["object"], "list");
        let data = v["data"].as_array().expect("data array");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "groq/m1");
        assert_eq!(data[0]["object"], "model");
        assert_eq!(data[0]["owned_by"], "combo");
        assert_eq!(data[0]["created"], 1_700_000_000u64);
    }

    #[test]
    fn parse_models_response_reads_openai_shape() {
        let body = r#"{"object":"list","data":[
            {"id":"m1","object":"model","created":1},
            {"id":"m2","object":"model"}
        ]}"#;
        let models = parse_models_response(body).expect("parse");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "m1");
        assert_eq!(models[0].created, Some(1));
        assert_eq!(models[1].id, "m2");
        assert_eq!(models[1].created, None);
    }

    #[test]
    fn parse_models_response_skips_garbage_and_dedups() {
        let body = r#"{"data":[
            {"id":"m1"},{"object":"model"},{"id":""},{"id":"m1"},{"id":"  m2  "}
        ]}"#;
        let models = parse_models_response(body).expect("parse");
        assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["m1", "m2"]);
    }

    #[test]
    fn parse_models_response_rejects_non_openai_payload() {
        assert!(parse_models_response("not json").is_err());
        assert!(parse_models_response(r#"{"object":"list"}"#).is_err());
    }

    #[test]
    fn parse_models_response_accepts_empty_list() {
        let models = parse_models_response(r#"{"data":[]}"#).expect("parse");
        assert!(models.is_empty());
    }
}