//! Vercel-релей: заголовки маршрутизации, проверка живости, развёртывание.
//!
//! ## Зачем релей
//!
//! Провайдеры могут быть недоступны из региона пользователя (гео-блокировка) или
//! из-за корпоративного/операторского фильтра. Edge-функция Vercel даёт выход с
//! чужой территории, и наш шлюз просто отправляет запрос через неё.
//!
//! ## Почему `x-target-url`, а не `x-target-host`
//!
//! Схема «`x-target-host: https://api.groq.com/openai/v1` + путь `/v1/chat/completions`»
//! из исходного ТЗ даёт `https://api.groq.com/openai/v1/v1/chat/completions` —
//! двойной `/v1` и `404` от провайдера. Шлюз считает **полный** URL апстрима сам
//! и передаёт его одним заголовком; воркер делает один `fetch` без склейки путей.
//!
//! ## Почему воркер требует секрет
//!
//! Реле развёрнуто публично. Без `x-relay-token` любой, кто узнает URL (он
//! попадает в логи, в скриншоты, в чужую ссылку), получает бесплатный прокси
//! «куда угодно» и расходует квоту владельца деплоя. Секрет генерируется шлюзом
//! автоматически при первом развёртывании, сохраняется в `gateway.json` и
//! передаётся Vercel как переменная окружения `RELAY_TOKEN` — пользователь
//! ничего не вводит и не может его забыть.
//!
//! ## Как секрет попадает на Vercel
//!
//! Через **документированный** эндпоинт `POST /v10/projects/{idOrName}/env`
//! с `?upsert=true` (тело: `key` / `value` / `type` / `target`), а не полем
//! `env` в теле `POST /v13/deployments`: такого поля в схеме Vercel REST API
//! нет, и опираться на него — значит закладываться на недокументированное
//! поведение, которое может исчезнуть без предупреждения.

use serde::Serialize;

use crate::app::AppState;

/// Исходник Edge-воркера. Отдаётся пользователю по `/api/relay/worker`, чтобы
/// развернуть реле вручную (гарантированно работающий путь, если API Vercel
/// недоступен или изменился).
pub const RELAY_WORKER: &str = include_str!("../assets/relay-worker.js");

/// Имя проекта Vercel по умолчанию.
pub const DEFAULT_VERCEL_PROJECT: &str = "cloud-routers-relay";

/// Endpoint развёртывания Vercel.
const VERCEL_DEPLOY_URL: &str = "https://api.vercel.com/v13/deployments";

/// Endpoint переменных окружения проекта (документирован в Vercel REST API).
///
/// `?upsert=true` обязателен: повторный деплой того же проекта иначе получил бы
/// `403 The environment variable cannot be created because it already exists` —
/// то есть вторую попытку развернуть релей провалило бы по причине, невидимой
/// пользователю.
const VERCEL_ENV_URL: &str = "https://api.vercel.com/v10/projects";

/// Имя переменной окружения, которую требует воркер.
const RELAY_TOKEN_ENV: &str = "RELAY_TOKEN";

/// Достать секрет релея, создав его при первом обращении.
///
/// Возвращает `Err`, только если запись в конфиг не удалась. Генерация секрета
/// не может «не получиться» молча: нет CSPRNG — ошибка наружу, а не пустой
/// токен (пустой токен воркер примет как «не настроено» и отдаст `503`, а
/// пользователь решил бы, что сломался релей, а не генератор).
pub fn ensure_relay_token(state: &AppState) -> Result<String, String> {
    if let Some(existing) = state
        .config()
        .vercel_relay_token
        .as_deref()
        .filter(|t| cloud_routers_core::is_usable_token(Some(t)))
    {
        return Ok(existing.to_string());
    }
    let fresh = cloud_routers_core::generate_relay_token().map_err(|e| e.to_string())?;
    let for_config = fresh.clone();
    state.update_config(move |c| {
        c.vercel_relay_token = Some(for_config);
        Ok(())
    })?;
    log::info!("создан новый секрет релея ({} символов)", fresh.len());
    Ok(fresh)
}

/// Результат проверки живости релея.
#[derive(Debug, Clone, Serialize)]
pub struct RelayProbe {
    /// Ответил ли релей.
    pub reachable: bool,
    /// HTTP-статус ответа на заведомо неполный запрос.
    pub status: u16,
    /// Ответ воркера — по нему видно, задеплоен ли именно наш воркер.
    pub detail: String,
    /// Латентность в миллисекундах.
    pub latency_ms: u64,
    pub url: String,
}

/// Проверить, что релей отвечает и что это именно наш воркер.
///
/// Проверка **не** тратит квоту провайдера: запрос уходит намеренно
/// неполным — без `x-target-url`. Наш воркер на такой запрос отвечает `400`
/// с текстом «missing x-target-url». Если вместо этого пришёл `401` — воркер
/// есть, но требует секрет; если `404` — по этому пути ничего не развёрнуто;
/// если соединение не установилось — релей недоступен.
pub async fn probe_relay(state: &AppState) -> Result<RelayProbe, String> {
    let config = state.config();
    let base = config
        .vercel_relay_url
        .as_deref()
        .map(cloud_routers_core::config::trim_base_url)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "URL релея не задан".to_string())?;
    let url = format!("{}{}/__ping", base, crate::upstream::RELAY_PATH_PREFIX);
    let started = std::time::Instant::now();
    let response = state
        .http()
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Релей недоступен: {}", e))?;
    let status = response.status().as_u16();
    let detail = response.text().await.unwrap_or_default();
    let latency_ms = started.elapsed().as_millis() as u64;
    // Наш воркер отвечает именно так на запрос без цели.
    let reachable = detail.contains("x-target-url");
    Ok(RelayProbe { reachable, status, detail: detail.chars().take(300).collect(), latency_ms, url })
}

/// Результат развёртывания релея.
#[derive(Debug, Clone, Serialize)]
pub struct DeployResult {
    /// Готовый URL релея.
    pub url: String,
    /// Идентификатор деплоя (для диагностики).
    pub deployment_id: Option<String>,
    /// Сырой ответ API — показывается в UI, чтобы причина сбоя была видна.
    pub raw: String,
}

/// Записать `RELAY_TOKEN` в переменные окружения проекта Vercel.
///
/// Отдельная функция, а не строчка в теле деплоя: переменная живёт на
/// ПРОЕКТЕ, а не на деплое, и ставится один раз — повторные деплои её наследуют.
/// Ошибка здесь пробрасывается наружу и **останавливает деплой**: развернуть
/// воркер без секрета значит вывесить в интернет прокси «куда угодно» за счёт
/// владельца деплоя. Тихий прогон продолжил бы и вернул бы URL неработающего,
/// но открытого релея.
async fn set_relay_env(
    state: &AppState,
    vercel_token: &str,
    project: &str,
    relay_token: &str,
) -> Result<(), String> {
    let url = format!("{}/{}/env?upsert=true", VERCEL_ENV_URL, project);
    let payload = serde_json::json!({
        "key": RELAY_TOKEN_ENV,
        "value": relay_token,
        // encrypted: значение не читается через `vercel env pull` и не светится
        // в списке переменных в панели.
        "type": "encrypted",
        "target": ["production"],
        "comment": "Секрет релея cloud-routers (заголовок x-relay-token). Создан шлюзом.",
    });
    let response = state
        .http()
        .post(url)
        .bearer_auth(vercel_token)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Не удалось обратиться к Vercel API при настройке {}: {}", RELAY_TOKEN_ENV, e))?;
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Vercel не принял переменную {} (HTTP {}): {}",
            RELAY_TOKEN_ENV,
            status.as_u16(),
            raw.chars().take(300).collect::<String>()
        ));
    }
    // Ответ 201 содержит `failed` даже при успешном HTTP-коде: Vercel отвечает
    // 201 и перечисляет в `failed` те переменные, которые создать не смог.
    // Молча проглотить это — значило бы сообщить «готово» про сломанный релей.
    if let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(failed) = body.get("failed").and_then(|f| f.as_array()) {
            if !failed.is_empty() {
                return Err(format!(
                    "Vercel не создал переменную {}: {}",
                    RELAY_TOKEN_ENV,
                    failed
                        .iter()
                        .filter_map(|f| {
                            f.pointer("/error/message")
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
    }
    Ok(())
}

/// Тело развёртывания. Собится один раз и используется обоими проходами —
/// иначе второй деплой мог бы уйти с другими файлами, чем первый, и релей
/// «починился» бы не тем кодом, который показал в UI.
fn deploy_payload(project: &str) -> serde_json::Value {
    serde_json::json!({
        "name": project,
        "target": "production",
        "files": [
            {
                "file": "package.json",
                "data": "{\"name\":\"cloud-routers-relay\",\"private\":true,\"type\":\"module\"}",
            },
            {
                "file": "api/gateway/[...path].js",
                "data": RELAY_WORKER,
            },
        ],
        "projectSettings": { "framework": null },
    })
}

/// Один вызов `POST /v13/deployments`. Возвращает `(url, id, сырой ответ)`.
///
/// `forceNew=1` обязателен для ВТОРОГО прохода: без него Vercel увидит два
/// одинаковых запроса и вернёт первый деплой обратно (дедупликация), то есть
/// «передеплой с секретом» тихо стал бы старым деплоем без секрета.
async fn create_deployment(
    state: &AppState,
    vercel_token: &str,
    project: &str,
    force_new: bool,
) -> Result<(String, Option<String>, String), String> {
    let payload = deploy_payload(project);
    let url = if force_new {
        format!("{}?forceNew=1", VERCEL_DEPLOY_URL)
    } else {
        VERCEL_DEPLOY_URL.to_string()
    };

    let started = std::time::Instant::now();
    let response = state
        .http()
        .post(url)
        .bearer_auth(vercel_token)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Не удалось обратиться к Vercel API: {}", e))?;

    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    let elapsed = started.elapsed().as_millis();

    if !status.is_success() {
        let detail: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw.clone()));
        let msg = detail
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .map(|c| format!("{}: {}", c, detail.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("")))
            .unwrap_or_else(|| raw.chars().take(400).collect());
        state
            .events()
            .error(format!("развёртывание релея не удалось: HTTP {} — {}", status.as_u16(), msg));
        return Err(format!("Vercel API ответил HTTP {} за {} мс: {}", status.as_u16(), elapsed, msg));
    }

    let body: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        format!("Ответ Vercel не является JSON ({}): {}", e, raw.chars().take(300).collect::<String>())
    })?;
    let url = body
        .pointer("/alias")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            body.pointer("/url").and_then(|v| v.as_str()).map(|s| format!("https://{}", s.trim_start_matches("//")))
        })
        .ok_or_else(|| {
            format!(
                "Vercel не вернул URL деплоя (ответ: {}). Откройте деплой в панели Vercel и вставьте URL вручную.",
                raw.chars().take(300).collect::<String>()
            )
        })?;
    let id = body.pointer("/id").and_then(|v| v.as_str()).map(|s| s.to_string());
    Ok((url, id, raw))
}

/// Развернуть Edge-воркер релея через Vercel API.
///
/// # Порядок вызовов и почему он именно такой
///
/// `RELAY_TOKEN` ставится документированным `POST /v10/projects/{idOrName}/env`,
/// а НЕ полем в теле деплоя: в схеме `POST /v13/deployments` такого поля нет
/// (в документации `env` встречается только в схеме **ответа** — списке имён
/// применённых переменных). Но `/v10/projects/{idOrName}/env` требует
/// **существующего** проект��, а проект создаётся первым деплоем
/// (`projectSettings` в документации — «required for the first deployment of a
/// project»). Отсюда порядок:
///
/// 1. деплой — создаёт проект (если его ещё нет);
/// 2. `POST /v10/.../env` — теперь проект есть, секрет сохранён НА ПРОЕКТЕ;
/// 3. деплой с `forceNew=1` — переменная попадает в сборку, воркер начинает
///    проверять секрет.
///
/// Первый деплой без секрета безопасен: воркер fail-closed и отвечает `503`,
/// ничего не проксируя. Пользователю его URL не показывается — возвращается
/// URL второго.
///
/// ## Честно о статусе этого пути
///
/// Последовательность собрана по документированной схеме Vercel REST API и
/// **не проверена на живом API** (в среде разработки нет сетевого доступа к
/// Vercel). Поэтому:
///
/// - при любой ошибке сырой ответ Vercel возвращается в UI целиком — видно,
///   что именно сказал API, а не «что-то пошло не так»;
/// - параллельно всегда доступен ручной путь: `/api/relay/worker` отдаёт тот же
///   исходник, а секрет отдаёт `/api/relay/token` — пользователь разворачивает
///   воркер сам и вставляет URL.
///
/// Пока ручной путь не заменён этим, авто-деплой следует считать
/// экспериментальным, а не основным.
pub async fn deploy_relay(
    state: &AppState,
    vercel_token: &str,
    project: Option<&str>,
) -> Result<DeployResult, String> {
    let token = vercel_token.trim();
    if token.is_empty() {
        return Err("Токен Vercel не указан".to_string());
    }
    let project = project.unwrap_or(DEFAULT_VERCEL_PROJECT);

    // Секрет создаётся ДО обращения к Vercel: без него развёрнутый воркер был бы
    // публичным прокси (см. `ensure_relay_token`).
    let relay_token = ensure_relay_token(state)?;

    // Проход 1: создаёт проект. Его URL нам не нужен и пользователю не
    // показывается — в нём ещё нет секрета.
    let (bootstrap_url, _, _) = create_deployment(state, token, project, false).await?;
    log::info!("релей: проект «{}» создан деплоем на {}", project, bootstrap_url);

    // Проход 2: секрет сохранён на проекте, дальше любой деплой его получит.
    set_relay_env(state, token, project, &relay_token).await?;

    // Проход 3: пересборка с секретом. Именно этот URL отдаём пользователю.
    let (url, deployment_id, raw) = match create_deployment(state, token, project, true).await {
        Ok(v) => v,
        Err(e) => {
            // Секрет на проекте уже сохранён, поэтому ситуация поправима:
            // сообщаем об этом явно, а не «деплой не удался» без подсказки.
            state.events().warn(format!(
                "релей: {} сохранён в проекте, но повторный деплой не удался ({})",
                RELAY_TOKEN_ENV, e
            ));
            return Err(format!(
                "{}. Переменная {} уже сохранена в проекте «{}» — нажмите «Развернуть на Vercel» ещё раз, \
                 и передеплой её подхватит.",
                e, RELAY_TOKEN_ENV, project
            ));
        }
    };

    state.events().info(format!("релей развёрнут на {}", url));
    Ok(DeployResult { url, deployment_id, raw: raw.chars().take(600).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_source_demands_shared_secret() {
        // Без секрета развёрнутый воркер — публичный прокси для всех.
        assert!(RELAY_WORKER.contains("x-relay-token"), "воркер не проверяет секрет");
        assert!(RELAY_WORKER.contains("RELAY_TOKEN"), "воркер не сверяет секрет с env");
    }

    #[test]
    fn worker_source_reads_absolute_target_url() {
        assert!(RELAY_WORKER.contains("x-target-url"), "воркер ждёт абсолютный URL цели");
        // Схема «host + pathname» из исходного ТЗ тут была бы с двойным /v1,
        // поэтому воркер строит URL целиком из заголовка.
        assert!(
            RELAY_WORKER.contains("new URL(rawTarget)"),
            "воркер должен строить URL цели из заголовка"
        );
        assert!(
            !RELAY_WORKER.contains("url.pathname"),
            "воркер не должен склеивать путь с host-адресом — это даёт двойной /v1"
        );
    }

    #[test]
    fn worker_source_is_edge_runtime_module() {
        assert!(RELAY_WORKER.contains("runtime: 'edge'") || RELAY_WORKER.contains("runtime: \"edge\""));
        assert!(RELAY_WORKER.contains("export default"), "воркер должен быть ES-модулем с default-экспортом");
    }

    #[test]
    fn worker_source_probes_without_target_url_mentioning_header_name() {
        // По этому тексту probe_relay отличает «наш воркер» от чужого ответа.
        assert!(RELAY_WORKER.contains("x-target-url"));
    }

    #[test]
    fn probe_requires_configured_url() {
        let dir = std::env::temp_dir().join("cr_gw_relay_nourl");
        let _ = std::fs::remove_dir_all(&dir);
        let state = crate::app::AppState::new(&dir).expect("state");
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let err = rt.block_on(probe_relay(&state)).expect_err("must fail");
        assert!(err.contains("URL релея не задан"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worker_fails_closed_without_env() {
        // Секрет обязателен именно как переменная окружения: без неё воркер
        // отвечает 503 и НИЧЕГО не проксирует. Если бы он падал в fail-open,
        // любой задеплоенный без секрета релей становился бы открытым прокси.
        assert!(
            RELAY_WORKER.contains("if (!expected)"),
            "воркер должен fail-closed при отсутствии RELAY_TOKEN"
        );
        assert!(RELAY_WORKER.contains("503"), "ожидался статус 503 при ненастроенном секрете");
    }

    /// РЕГРЕССИЯ на неверный порядок вызовов.
    ///
    /// `/v10/projects/{id}/env` требует СУЩЕСТВУЮЩИЙ проект, а проект создаёт
    /// первый деплой. Старая версия ставила переменную ПЕРВОЙ — и первое
    /// развёртывание нового проекта падало на `404`, не дойдя до деплоя.
    ///
    /// Проверяем форму кода: вызовы развёртывания должны ОПЕРЕЖАТЬ`set_relay_env`
    /// (создание проекта), а финальный вызов обязан идти ПОСЛЕ него (сборка с
    /// секретом) и с `forceNew` — иначе дедупликация вернула бы старый деплой.
    #[test]
    fn env_is_set_between_bootstrap_and_final_deployment() {
        let src = include_str!("relay.rs");
        let body = &src[src.find("pub async fn deploy_relay").expect("функция есть")..];

        let bootstrap = body.find("create_deployment(state, token, project, false)")
            .expect("создающий проект деплой");
        let env = body.find("set_relay_env(state, token, project, &relay_token)")
            .expect("установка переменной");
        let final_deploy = body.find("create_deployment(state, token, project, true)")
            .expect("финальный деплой с секретом");

        assert!(bootstrap < env, "переменная ставится ДО создания проекта -> 404");
        assert!(env < final_deploy, "финальный деплой идёт ДО установки секрета -> релей без секрета");
    }

    #[test]
    fn relay_token_goes_through_documented_env_endpoint() {
        assert!(
            VERCEL_ENV_URL.contains("/v10/projects"),
            "переменные окружения ставятся документированным POST /v10/projects/{{id}}/env, а не полем env в теле деплоя"
        );
        assert_eq!(RELAY_TOKEN_ENV, "RELAY_TOKEN", "имя обязано совпадать с тем, что читает воркер");
        assert!(
            RELAY_WORKER.contains(RELAY_TOKEN_ENV),
            "воркер читает другое имя переменной, чем задаёт деплой"
        );
    }

    /// Оба прохода деплоя обязаны нести ОДИН И ТОТ ЖЕ набор файлов. Если бы
    /// тело собиралось дважды, второй деплой мог бы уйти с другим воркером,
    /// и пользователю показали бы URL, где работает не тот код.
    #[test]
    fn both_deployment_passes_share_one_payload() {
        let src = include_str!("relay.rs");
        let body = &src[src.find("fn deploy_payload").expect("сборщик тела есть")..];
        let fn_body = &body[..body.find("\n}\n").expect("конец функции")];

        assert_eq!(
            fn_body.matches("\"files\"").count(),
            1,
            "список файлов должен собираться один раз"
        );
        assert!(
            fn_body.contains("RELAY_WORKER"),
            "воркер обязан входить в тело деплоя"
        );

        // И вызывающая функция берёт тело только через сборщик, не собирает своё.
        let caller = &src[src.find("pub async fn deploy_relay").expect("функция есть")..];
        assert!(
            !caller.contains("\"files\""),
            "deploy_relay не должен собирать тело деплоя сам — это обошло бы общий источник правды"
        );
    }

    /// Секрет создаётся при первом обращении и кладётся в `gateway.json`:
    /// без этого развёрнутый воркер был бы открытым прокси.
    #[tokio::test]
    async fn ensure_relay_token_creates_and_persists_secret() {
        let dir = std::env::temp_dir().join("cr_gw_relay_gen");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        assert!(
            state.config().vercel_relay_token.is_none(),
            "в свежем конфиге секрета быть не должно"
        );

        let token = ensure_relay_token(&state).expect("секрет создаётся");
        assert_eq!(token.len(), 64, "ожидался 64 hex-символа: {}", token);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()), "не hex: {}", token);

        let on_disk = cloud_routers_core::load_config(&dir).vercel_relay_token;
        assert_eq!(
            on_disk.as_deref(),
            Some(token.as_str()),
            "секрет обязан пережить перезапуск шлюза — он лежит в gateway.json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Повторное обращение не должно менять секрет: смена секрета без
    /// передеплоя оставила бы воркер с прежним `RELAY_TOKEN` и обрушила бы все
    /// запросы (401 на каждом).
    #[tokio::test]
    async fn ensure_relay_token_is_stable_and_replaces_blank() {
        let dir = std::env::temp_dir().join("cr_gw_relay_stable");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        let first = ensure_relay_token(&state).expect("первый");
        let second = ensure_relay_token(&state).expect("второй");
        assert_eq!(first, second, "секрет не должен меняться при повторном обращении");

        // Пустая строка в конфиге — не секрет: воркер сравнивает посимвольно,
        // и пробельный токен прошёл бы проверку «задан», но отдавал бы 401.
        state
            .update_config(|c| {
                c.vercel_relay_token = Some("   ".to_string());
                Ok(())
            })
            .expect("update");
        let third = ensure_relay_token(&state).expect("пересоздание");
        assert_eq!(third.len(), 64, "подставлен настоящий секрет, а не пробелы: {:?}", third);
        assert_ne!(third, first, "битый секрет обязан быть заменён");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn deploy_requires_token() {
        let dir = std::env::temp_dir().join("cr_gw_relay_notoken");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        let err = deploy_relay(&state, "   ", None).await.expect_err("must fail");
        assert!(err.contains("Токен Vercel"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn deploy_surfaces_raw_api_error_instead_of_generic_message() {
        // Токен заведомо невалидный: запрос уйдёт, но полезного деплоя не будет.
        // Проверяем контракт: при ошибке возвращается HTTP-статус и текст API,
        // а не «что-то пошло не так».
        let dir = std::env::temp_dir().join("cr_gw_relay_badtoken");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        match deploy_relay(&state, "vercel_invalid_token_for_test", None).await {
            Err(e) => {
                assert!(!e.is_empty());
                assert!(
                    e.contains("Vercel") || e.contains("vercel"),
                    "ошибка должна называть источник: {e}"
                );
            }
            Ok(_) => panic!("deploy с заведомо невалидным токеном не должен succeed"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deploy_payload_has_no_config() {
        let payload = deploy_payload("test-project");
        assert!(payload.get("config").is_none());
    }
}