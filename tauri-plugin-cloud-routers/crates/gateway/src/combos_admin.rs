//! CRUD пользовательских комбо-связок (`/api/combos/config*`).
//!
//! Вынесен в отдельный модуль по правилу размера файлов: `admin.rs` уже
//! перевалил за 3000 строк, и добавлять туда ещё один домен было бы
//! нельзя. Единая точка валидации связки — [`GatewayConfig::validate`] через
//! [`ComboConfig::validate_against`]: CRUD проверяет тем же кодом, что и
//! загрузка конфига, иначе появились бы две правды о «валидной связке».
//!
//! ## Секреты
//!
//! Связки не содержат секретов: только id ключей (`key_ids`). Ответы отдают
//! конфигурацию связок как есть — маскировать нечего (маскирует ключи
//! `admin.rs::provider_dto`, здесь этого не требуется).

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use cloud_routers_core::config::{ComboConfig, ComboMember};

use crate::admin::{bad_request, internal, not_found, notify_host};
use crate::app::AppState;

/// Тело `PUT /api/combos/config/:name` — участники новой/заменяемой связки.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ComboBody {
    members: Vec<ComboMember>,
}

/// Список пользовательских связок в порядке конфига (он же порядок в списке
/// моделей). Секретов нет — id ключей не секрет.
async fn list_user_combos(State(state): State<AppState>) -> Response {
    let config = state.config();
    Json(serde_json::json!({ "combos": config.combos })).into_response()
}

/// Создать или заменить связку с именем из пути.
///
/// Имя — из пути (не из тела): путь — адрес ресурса, тело — содержимое.
/// Замена одноимённой связки разрешена осознанно — это PUT, а не «только
/// создать»; предварительная валидация даёт 400 с конкретной причиной, запись
/// на диск идёт через [`AppState::update_config`] (валидация внутри — барьер
/// на случай гонки двух правок).
async fn put_combo(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<ComboBody>,
) -> Result<Response, Response> {
    let combo = ComboConfig { name: name.trim().to_string(), members: body.members };
    // Предварительная валидация по снимку: ошибка содержимого — 400 с
    // перечнем проблем, а не 500 от записи на диск.
    let problems = combo.validate_against(&state.config().providers);
    if !problems.is_empty() {
        return Err(bad_request(problems.join("; ")));
    }
    state
        .update_config(|c| {
            c.combos.retain(|x| x.name != combo.name);
            c.combos.push(combo.clone());
            let problems = c.validate();
            if !problems.is_empty() {
                // Черновик будет выброшен — файл и память не изменятся.
                return Err(problems.join("; "));
            }
            Ok(())
        })
        .map_err(internal)?;
    state.events().info(format!(
        "сохранена комбо-связка «{}» (участников: {})",
        combo.name,
        combo.members.len()
    ));
    notify_host(&state, "combo-saved");
    Ok(Json(combo).into_response())
}

/// Удалить связку по имени. Несуществующее имя — 404: «уже удалено» и «не
/// существовало» неразличимы только изнутри, пользователь должен знать правду.
async fn delete_combo(State(state): State<AppState>, Path(name): Path<String>) -> Result<Response, Response> {
    let name = name.trim().to_string();
    let existed = state.config().combos.iter().any(|c| c.name == name);
    if !existed {
        return Err(not_found(format!("комбо-связка «{}» не найдена", name)));
    }
    state
        .update_config(|c| {
            c.combos.retain(|x| x.name != name);
            Ok(())
        })
        .map_err(internal)?;
    state.events().info(format!("удалена комбо-связка «{}»", name));
    notify_host(&state, "combo-deleted");
    Ok(Json(serde_json::json!({ "ok": true })).into_response())
}

/// Роутер CRUD связок. Синтаксис параметров — axum 0.7 (`:name`), как и в
/// `admin.rs` (см. регрессию `provider_param_routes_use_axum07_syntax`).
pub fn router(state: AppState) -> axum::Router<AppState> {
    axum::Router::new()
        .route("/api/combos/config", axum::routing::get(list_user_combos))
        .route(
            "/api/combos/config/:name",
            axum::routing::put(put_combo).delete(delete_combo),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::admin::log_admin_errors,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use cloud_routers_core::config::{AccountKey, DiscoveredModel, GatewayConfig, ProviderConfig};
    use tower::ServiceExt;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cr_gw_combos_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn provider(id: &str, models: &[&str], keys: &[&str]) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: id.into(),
            base_url: format!("https://{id}.example.com/v1"),
            is_enabled: true,
            relay_id: None,
            keys: keys
                .iter()
                .map(|k| AccountKey { id: k.to_string(), key: format!("sk-{k}"), is_active: true, name: None })
                .collect(),
            models: models
                .iter()
                .map(|m| DiscoveredModel { id: m.to_string(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    fn state_with(dir: &std::path::Path) -> AppState {
        std::fs::create_dir_all(dir).expect("mkdir");
        let mut cfg = GatewayConfig::default();
        cfg.providers.push(provider("groq", &["m1"], &["k1", "k2"]));
        cfg.providers.push(provider("deep", &["d1"], &["k3"]));
        cloud_routers_core::save_config(dir, &cfg).expect("save");
        AppState::new(dir).expect("state")
    }

    async fn call(app: axum::Router, method: &str, uri: &str, body: Option<serde_json::Value>) -> (axum::http::StatusCode, serde_json::Value) {
        let builder = Request::builder().method(method).uri(uri);
        let req = match body {
            Some(v) => builder
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&v).expect("ser")))
                .expect("req"),
            None => builder.body(Body::empty()).expect("req"),
        };
        let r = app.oneshot(req).await.expect("response");
        let status = r.status();
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.expect("body");
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    fn app(dir: &std::path::Path) -> axum::Router {
        crate::server::build(AppState::new(dir).expect("state"))
    }

    #[tokio::test]
    async fn put_creates_list_and_delete_removes() {
        let dir = tmp("crud");
        let st = state_with(&dir);
        let a = app(&dir);

        let (status, v) = call(
            a.clone(),
            "PUT",
            "/api/combos/config/fast",
            Some(serde_json::json!({
                "members": [
                    {"provider_id": "groq", "model_id": "m1", "key_ids": ["k2"]},
                    {"provider_id": "deep", "model_id": "d1"}
                ]
            })),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::OK, "{v}");
        assert_eq!(v["name"], "fast");
        assert_eq!(v["members"][0]["key_ids"][0], "k2");

        let (_, v) = call(a.clone(), "GET", "/api/combos/config", None).await;
        assert_eq!(v["combos"].as_array().expect("combos").len(), 1);
        assert_eq!(v["combos"][0]["name"], "fast");

        // Связка видна и в списке моделей (то же SSOT-построение).
        let (_, v) = call(a.clone(), "GET", "/api/combos", None).await;
        let names: Vec<&str> = v["combos"]
            .as_array()
            .expect("combos")
            .iter()
            .map(|c| c["name"].as_str().expect("name"))
            .collect();
        assert!(names.contains(&"fast"), "{names:?}");

        let (status, v) = call(a, "DELETE", "/api/combos/config/fast", None).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{v}");
        let (_, v) = call(app(&dir), "GET", "/api/combos/config", None).await;
        assert_eq!(v["combos"].as_array().expect("combos").len(), 0);
        // Конфиг на диске согласован с памятью.
        assert!(cloud_routers_core::load_config(&dir).combos.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = st;
    }

    #[tokio::test]
    async fn put_replaces_existing_combo_with_same_name() {
        let dir = tmp("replace");
        let _ = state_with(&dir);
        let a = app(&dir);
        let body = |members: serde_json::Value| {
            serde_json::json!({ "members": members })
        };
        let (s1, _) = call(
            a.clone(),
            "PUT",
            "/api/combos/config/fast",
            Some(body(serde_json::json!([{"provider_id": "groq", "model_id": "m1"}]))),
        )
        .await;
        let (s2, v2) = call(
            a,
            "PUT",
            "/api/combos/config/fast",
            Some(body(serde_json::json!([{"provider_id": "deep", "model_id": "d1"}]))),
        )
        .await;
        assert_eq!(s1, axum::http::StatusCode::OK);
        assert_eq!(s2, axum::http::StatusCode::OK, "{v2}");
        let cfg = cloud_routers_core::load_config(&dir);
        assert_eq!(cfg.combos.len(), 1, "замена, а не дубликат: {:?}", cfg.combos);
        assert_eq!(cfg.combos[0].members[0].provider_id, "deep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_rejects_invalid_members_with_reason_and_keeps_disk_clean() {
        let dir = tmp("reject");
        let _ = state_with(&dir);
        let a = app(&dir);
        let cases: Vec<(&str, serde_json::Value, &str)> = vec![
            (
                "несуществующий провайдер",
                serde_json::json!([{"provider_id": "ghost", "model_id": "m1"}]),
                "ghost",
            ),
            (
                "не обнаруженная модель",
                serde_json::json!([{"provider_id": "groq", "model_id": "nope"}]),
                "не обнаружена",
            ),
            (
                "пустой список участников",
                serde_json::json!([]),
                "без участников",
            ),
            (
                "ключ-призрак",
                serde_json::json!([{"provider_id": "groq", "model_id": "m1", "key_ids": ["ghost"]}]),
                "не существует",
            ),
            (
                "пустой список ключей",
                serde_json::json!([{"provider_id": "groq", "model_id": "m1", "key_ids": []}]),
                "пустой список key_ids",
            ),
        ];
        for (what, members, needle) in cases {
            let (status, v) = call(a.clone(), "PUT", "/api/combos/config/fast", Some(serde_json::json!({ "members": members })))
                .await;
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{what}: {v}");
            let msg = v["error"].as_str().unwrap_or_default();
            assert!(msg.contains(needle), "{what}: ожидали «{needle}» в «{msg}»");
        }
        // Слэш в имени структурно невозможен: параметр `:name` матчит ровно
        // один сегмент пути, поэтому «fast/llama» не доходит до обработчика и
        // честно отдаёт 404 (совпадений маршрута нет). Валидация имени
        // подстраховывает только URL-кодированный слэш (%2F), который
        // декодируется уже внутри параметра.
        let (status, _v) = call(
            a,
            "PUT",
            "/api/combos/config/fast/llama",
            Some(serde_json::json!({ "members": [{"provider_id": "groq", "model_id": "m1"}] })),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
        // Ни одна из попыток не изменила конфиг на диске.
        assert!(cloud_routers_core::load_config(&dir).combos.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn delete_unknown_combo_is_404_with_reason() {
        let dir = tmp("del404");
        let _ = state_with(&dir);
        let (status, v) = call(app(&dir), "DELETE", "/api/combos/config/nope", None).await;
        assert_eq!(status, axum::http::StatusCode::NOT_FOUND, "{v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("не найдена"), "{v}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
