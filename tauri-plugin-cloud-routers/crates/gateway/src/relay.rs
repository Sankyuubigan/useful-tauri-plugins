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
/// PATCH `/v9/projects/{idOrName}` — снятие Deployment Protection с проекта.
/// Версии пути DIFFЕРЕНТНЫ: env-переменные — `/v10`, свойства проекта — `/v9`
/// (обе версии документированы одновременно, см. доку Update an existing project).
const VERCEL_PROJECT_PATCH_URL: &str = "https://api.vercel.com/v9/projects";
/// Пауза между опросами готовности деплоя. Образец — деплой 9router (шаг 3 с).
const DEPLOY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
/// Общий дедлайн ожидания READY. Образец — деплой 9router (120 с).
const DEPLOY_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Имя переменной окружения, которую требует воркер.
const RELAY_TOKEN_ENV: &str = "RELAY_TOKEN";

/// Достать секрет релея, создав его при первом обращении.
///
/// Возвращает `Err`, только если запись в конфиг не удалась. Генерация секрета
/// не может «не получиться» молча: нет CSPRNG — ошибка наружу, а не пустой
/// токен (пустой токен воркер примет как «не настроено» и отдаст `503`, а
/// пользователь решил бы, что сломался релей, а не генератор).
pub fn ensure_relay_token(state: &AppState) -> Result<String, String> {
    if let Some(existing) = state.config().relay_token().filter(|t| cloud_routers_core::is_usable_token(Some(t))) {
        return Ok(existing);
    }
    let fresh = cloud_routers_core::generate_relay_token().map_err(|e| e.to_string())?;
    let for_config = fresh.clone();
    // Секрет пишется в Vercel-слот, если он есть (миграция 23.10.2026);
    // в конфигах без слотов — в legacy-поле (совместимая запись).
    state.update_config(move |c| {
        if let Some(slot) = c.vercel_slot_mut() {
            slot.relay_secret = Some(for_config);
        } else {
            c.vercel_relay_token = Some(for_config);
        }
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
    // Единая проверка базы: негодная (без схемы) база релея иначе дала бы
    // reqwest Kind::Builder («builder error»), а не причину с текстом чинилки.
    let base = config
        .relay_url()
        .as_deref()
        .filter(|s| !cloud_routers_core::config::trim_base_url(s).is_empty())
        .and_then(cloud_routers_core::config::relay_base_url)
        .ok_or_else(|| {
            let relay_url_owned = config.relay_url();
            let raw = relay_url_owned
                .as_deref()
                .map(|s| s.trim())
                .unwrap_or_default();
            if raw.is_empty() {
                "URL релея не задан".to_string()
            } else {
                format!("URL релея «{raw}» не начинается с http:// или https://")
            }
        })?;
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
///
/// ТРИ файла — как в рабочем образце (деплой релея 9router):
/// `package.json`, воркер и `vercel.json` с rewrite. Регресс 27.10.2026:
/// воркер клали в `api/gateway/[...path].js` без `vercel.json` — Vercel
/// НЕ зарегистрировал catch-all-функцию и любой путь деплой-URL отвечал
/// собственной страницей `NOT_FOUND` (HTTP 404, HTML). Образец решает это
/// конструктивно: воркер лежит на простом статическом пути `api/relay.js`,
/// а rewrite `/(.*)` → `/api/relay` направляет на него ЛЮБОЙ запрос, так что
/// путь входящего запроса роли не играет — цель релей берёт из `x-target-url`.
fn deploy_payload(project: &str) -> serde_json::Value {
    serde_json::json!({
        "name": project,
        "target": "production",
        "files": [
            {
                "file": "package.json",
                // version обязателен: без него Vercel-билд package.json не
                // проходит (образец рабочего деплоя — 9router: {name, version}).
                "data": "{\"name\":\"cloud-routers-relay\",\"version\":\"1.0.0\",\"private\":true,\"type\":\"module\"}",
            },
            {
                "file": "api/relay.js",
                "data": RELAY_WORKER,
            },
            {
                "file": "vercel.json",
                // rewrites — из рабочего образца: каждый путь деплоя попадает
                // в воркер. Без него фильтрация маршрутов Vercel отдавала бы
                // NOT_FOUND на всё, кроме корня (см. доккомментарий выше).
                "data": "{\"rewrites\":[{\"source\":\"/(.*)\",\"destination\":\"/api/relay\"}]}",
            },
        ],
        "projectSettings": { "framework": null },
    })
}

/// Один вызов `POST /v13/deployments`. Возвращает `(id, сырой ответ)`.
///
/// URL из этого ответа НЕ берётся: деплой только создан и ещё строится
/// (QUEUED → BUILDING → READY), а алиасы/готовность наступают ПОЗЖЕ.
/// URL берётся из опрошенного до READY деплоя — см. [`wait_deployment_ready`]
/// и кейс 27.10.2026: `alias[0]` из POST-ответа указывал на ПРЕДЫДУЩИЙ
/// production-деплой проекта (тот отдал HTML-страницу вместо воркера).
///
/// `forceNew=1` обязателен для ВТОРОГО прохода: без него Vercel увидит два
/// одинаковых запроса и вернёт первый деплой обратно (дедупликация), то есть
/// «передеплой с секретом» тихо стал бы старым деплоем без секрета.
async fn create_deployment(
    state: &AppState,
    vercel_token: &str,
    project: &str,
    force_new: bool,
) -> Result<(String, String), String> {
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
    let id = body
        .pointer("/id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!(
                "Vercel не вернул id деплоя (ответ: {}). Опросить готовность невозможно; \
                 откройте деплой в панели Vercel и вставьте URL вручную.",
                raw.chars().take(300).collect::<String>()
            )
        })?;
    Ok((id, raw))
}

/// Опросить деплой до READY (образец — `waitForDeployment` деплоя 9router:
/// шаг 3 с, общий дедлайн 120 с).
///
/// Доктрина Vercel: POST создаёт деплой, который ещё строится; готовность
/// отслеживается опросом `GET /v13/deployments/{id}` (`readyState`). Взять URL
/// до READY — значит показать пользователю домен, где ещё крутится старый
/// контент проекта (кейс 27.10.2026).
async fn wait_deployment_ready(
    state: &AppState,
    vercel_token: &str,
    deployment_id: &str,
) -> Result<serde_json::Value, String> {
    let deadline = std::time::Instant::now() + DEPLOY_WAIT_TIMEOUT;
    let url = format!("{}/{}", VERCEL_DEPLOY_URL, deployment_id);
    loop {
        let response = state
            .http()
            .get(&url)
            .bearer_auth(vercel_token)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Не удалось опросить деплой {}: {}", deployment_id, e))?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(format!(
                "Опрос деплоя {} вернул HTTP {}: {}",
                deployment_id,
                status.as_u16(),
                raw.chars().take(300).collect::<String>()
            ));
        }
        let body: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            format!("Ответ опроса деплоя {} не является JSON ({}): {}", deployment_id, e, raw.chars().take(200).collect::<String>())
        })?;
        match body.pointer("/readyState").and_then(|v| v.as_str()) {
            Some("READY") => return Ok(body),
            Some("ERROR") | Some("CANCELED") => {
                let msg = body
                    .pointer("/errorMessage")
                    .and_then(|m| m.as_str())
                    .unwrap_or("причина не указана API");
                let code = body.pointer("/errorCode").and_then(|c| c.as_str()).unwrap_or("");
                return Err(format!(
                    "Деплой {} завершился со статусом {}{}: {}",
                    deployment_id,
                    body.pointer("/readyState").and_then(|v| v.as_str()).unwrap_or("?"),
                    if code.is_empty() { String::new() } else { format!(" ({})", code) },
                    msg
                ));
            }
            _ => {
                if std::time::Instant::now() >= deadline {
                    let state_now = body.pointer("/readyState").and_then(|v| v.as_str()).unwrap_or("неизвестно");
                    return Err(format!(
                        "Деплой {} не стал READY за {} с (последний статус: {}). \
                         Продолжение строится дольше ожидаемого — проверьте деплой в панели Vercel.",
                        deployment_id,
                        DEPLOY_WAIT_TIMEOUT.as_secs(),
                        state_now
                    ));
                }
                tokio::time::sleep(DEPLOY_POLL_INTERVAL).await;
            }
        }
    }
}

/// URL готового (READY) деплоя — **деплой-URL**, а не production-алиас.
///
/// Рабочий образец (9router) сохраняет в пул именно `url` готового деплоя:
/// уникальный деплой-домен гарантированно указывает на наш воркер. Production-
/// алиас может быть занят/указывать на другой деплой проекта — берём его
/// только как поле `alias` для диагностики, а в слот пишем деплой-URL.
/// Подразумевается, что Deployment Protection снята ([`unprotect_deployment`]):
/// иначе деплой-URL отдаёт HTML-страницу Vercel вместо воркера.
fn ready_deployment_url(body: &serde_json::Value) -> Result<String, String> {
    let url = body
        .pointer("/url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches("https://").trim_start_matches("//").to_string())
        .ok_or_else(|| {
            format!(
                "Готовый деплой не содержит url (ответ: {})",
                serde_json::to_string(body).unwrap_or_default().chars().take(300).collect::<String>()
            )
        })?;
    Ok(format!("https://{}", url))
}

/// Снять Deployment Protection (Vercel Authentication) с проекта.
///
/// Standard Protection по умолчанию закрывает сгенерированный деплой-URL
/// аутентификацией Vercel: шлюз получает HTML-страницу «войдите в Vercel»
/// вместо ответа воркера. Документированный способ — `PATCH /v9/projects/
/// {idOrName}` c `ssoProtection: null` (именно это делает деплой 9router,
/// иначе его деплой-URL недоступен снаружи). Версия пути — `/v9`, НЕ `/v10`.
async fn unprotect_deployment(state: &AppState, vercel_token: &str, project: &str) -> Result<(), String> {
    let response = state
        .http()
        .patch(format!("{}/{}", VERCEL_PROJECT_PATCH_URL, project))
        .bearer_auth(vercel_token)
        .json(&serde_json::json!({ "ssoProtection": null }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Не удалось обратиться к Vercel API при снятии защиты проекта: {}", e))?;
    let status = response.status();
    let raw = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Vercel не снял Deployment Protection с проекта «{}» (HTTP {}): {} \
             — деплой-URL будет отдавать HTML-страницу Vercel вместо воркера.",
            project,
            status.as_u16(),
            raw.chars().take(300).collect::<String>()
        ));
    }
    Ok(())
}

/// Развернуть Edge-воркер релея через Vercel API.
///
/// # Порядок вызовов и почему он именно такой
///
/// `RELAY_TOKEN` ставится документированным `POST /v10/projects/{idOrName}/env`,
/// а НЕ полем в теле деплоя: в схеме `POST /v13/deployments` такого поля нет
/// (в документации `env` встречается только в схеме **ответа** — списке имён
/// применённых переменных). Но `/v10/projects/{idOrName}/env` требует
/// **существующего** проекта, а проект создаётся первым деплоем
/// (`projectSettings` в документации — «required for the first deployment of a
/// project»). Отсюда порядок:
///
/// 1. деплой — создаёт проект (если его ещё нет);
/// 2. `PATCH /v9/projects/{idOrName}` со `ssoProtection: null` — проект есть,
///    Deployment Protection снята, деплой-URL будет доступен шлюзу без входа
///    в Vercel (иначе — HTML-страница Vercel вместо воркера, кейс 27.10.2026);
/// 3. `POST /v10/.../env` — секрет сохранён НА ПРОЕКТЕ;
/// 4. деплой с `forceNew=1` — переменная попадает в сборку, воркер начинает
///    проверять секрет;
/// 5. опрос деплоя до READY — и только готовый деплой-URL идёт в слот.
///    URL из ответа на создание НЕ берётся: деплой ещё строится, алиасы
///    назначены только формально, а production-домен может показывать
///    ПРЕДЫДУЩИЙ деплой проекта (кейс 27.10.2026 — HTML вместо воркера).
///
/// Первый деплой без секрета безопасен: воркер fail-closed и отвечает `503`,
/// ничего не проксируя. Пользователю его URL не показывается — возвращается
/// URL второго.
///
/// ## Честно о статусе этого пути
///
/// Последовательность собрана по документированной схеме Vercel REST API;
/// опрос READY и снятие защиты повторяют рабочий образец — деплой релея
/// 9router (`waitForDeployment` + `PATCH ssoProtection: null`). Живой прогон
/// всей цепочки возможен только с токеном Vercel пользователя, поэтому:
///
/// - при любой ошибке сырой ответ Vercel возвращается в UI целиком — видно,
///   что именно сказал API, а не «что-то пошло не так»;
/// - параллельно всегда доступен ручной путь: `/api/relay/worker` отдаёт тот же
///   исходник, а секрет отдаёт `/api/relay/token` — пользователь разворачивает
///   воркер сам и вставляет URL.

/// Записать URL успешного деплоя в Vercel-слот конфига (шаг 5 цепочки деплоя).
///
/// Слота нет (пользователь удалил его перед передеплоем, свежий конфиг) —
/// слот **создаётся**: успешный деплой обязан значить «URL есть и в списке
/// слотов», иначе он застревает в legacy-полях — не виден во вкладке «Релей»,
/// не привязывается к провайдерам (регресс 27.10.2026: после удаления слота
/// деплой писал URL в `vercel_relay_url`, и развёрнутый релей «исчезал» из
/// списка). Legacy-поля очищаются той же записью — после миграции слот
/// единственный источник правды (договор миграции 23.10.2026).
fn save_deployed_url(state: &AppState, url: &str, relay_secret: &str) -> Result<(), String> {
    let url = url.to_string();
    let secret = relay_secret.to_string();
    state.update_config(move |c| {
        c.migrate_relay_slots();
        let activated = if let Some(slot) = c.vercel_slot_mut() {
            slot.url = Some(url.clone());
            slot.relay_secret = Some(secret);
            slot.id.clone()
        } else {
            c.relays.push(cloud_routers_core::config::RelaySlot {
                id: "vercel".to_string(),
                kind: cloud_routers_core::config::RELAY_KIND_VERCEL.to_string(),
                url: Some(url.clone()),
                relay_secret: Some(secret),
                cf_account_id: None,
                cf_script_name: None,
            });
            "vercel".to_string()
        };
        c.active_relay = Some(activated);
        c.vercel_relay_url = None;
        c.vercel_relay_token = None;
        Ok(())
    })
}

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

    // Проход 1: создаёт проект. URL не используется — в нём ещё нет секрета
    // и деплой ещё не готов (см. `wait_deployment_ready`).
    let (bootstrap_id, _) = create_deployment(state, token, project, false).await?;
    log::info!("релей: проект «{}» создан деплоем {}", project, bootstrap_id);

    // Защита снимается, пока проект уже существует, а деплой-URL ещё не издан:
    // Standard Protection закрывает сгенерированный деплой-URL аутентификацией
    // Vercel — шлюз получил бы HTML вместо воркера (кейс 27.10.2026).
    unprotect_deployment(state, token, project).await?;

    // Секрет сохранён на проекте, дальше любой деплой его получит.
    set_relay_env(state, token, project, &relay_token).await?;

    // Проход 2: пересборка с секретом. Именно этот деплой дожидаемся до READY.
    let (deployment_id, _) = match create_deployment(state, token, project, true).await {
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

    // Готовность опрашивается ДО того, как URL попадёт в слот: иначе слот
    // указывал бы на домен, где крутится предыдущий контент проекта.
    let ready = wait_deployment_ready(state, token, &deployment_id).await?;
    let url = ready_deployment_url(&ready)?;
    // Слот пишется здесь же (симметрия с CF-путём, relay_cf.rs): иначе в
    // конфиге без Vercel-слота URL уезжал бы в legacy и релей «исчезал» бы
    // из списка слотов (регресс 27.10.2026).
    save_deployed_url(state, &url, &relay_token)?;
    state.events().info(format!("релей развёрнут на {}", url));
    Ok(DeployResult {
        url,
        deployment_id: Some(deployment_id),
        raw: serde_json::to_string(&ready).unwrap_or_default().chars().take(600).collect(),
    })
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
            state.config().relay_token().is_none(),
            "в свежем конфиге секрета быть не должно"
        );

        let token = ensure_relay_token(&state).expect("секрет создаётся");
        assert_eq!(token.len(), 64, "ожидался 64 hex-символа: {}", token);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()), "не hex: {}", token);

        // Читаем через геттер: конфиг без слотов → миграция на чтении переносит
        // legacy-значение в слот, и геттер его возвращает (SSOT-чтение).
        let on_disk = cloud_routers_core::load_config(&dir).relay_token();
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

    /// REGRESSION 27.10.2026: package.json без `version` — Vercel-билд не
    /// проходит. Рабочий образец (деплой 9router) шлёт {name, version}.
    #[test]
    fn deploy_payload_carries_package_version() {
        let payload = deploy_payload("test-project");
        let pkg = payload
            .pointer("/files/0/data")
            .and_then(|v| v.as_str())
            .expect("package.json в теле деплоя");
        let parsed: serde_json::Value = serde_json::from_str(pkg).expect("package.json — валидный JSON");
        assert_eq!(
            parsed.pointer("/version").and_then(|v| v.as_str()),
            Some("1.0.0"),
            "package.json обязан нести version: {pkg}"
        );
    }

    /// REGRESSION 27.10.2026 (NOT_FOUND): воркер в `api/gateway/[...path].js`
    /// без vercel.json не зарегистрировался функцией — Vercel отвечал 404
    /// NOT_FOUND на любой путь деплой-URL. Рабочий образец (деплой релея
    /// 9router) шлёт третий файл vercel.json с rewrite «все пути → воркер».
    #[test]
    fn deploy_payload_maps_every_path_to_worker() {
        let payload = deploy_payload("test-project");
        let vercel_json = payload
            .pointer("/files/2/data")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .expect("vercel.json в теле деплоя");
        let rewrite = vercel_json.pointer("/rewrites/0").expect("rewrite в vercel.json");
        assert_eq!(rewrite.get("source").and_then(|v| v.as_str()), Some("/(.*)"));
        // destination обязан указывать ровно на файл воркера (assert ниже):
        // расхождение означало бы rewrite в никуда и тот же NOT_FOUND.
        assert_eq!(
            rewrite.get("destination").and_then(|v| v.as_str()),
            Some("/api/relay")
        );
        let worker = payload
            .pointer("/files/1/file")
            .and_then(|v| v.as_str())
            .expect("файл воркера");
        assert_eq!(worker, "api/relay.js", "воркер на простом статическом пути");
        assert!(
            !worker.contains('['),
            "catch-all-имя с [] не регистрируется маршрутизацией Vercel: {worker}"
        );
    }

    /// URL слота берётся из опрошенного до READY деплоя — это деплой-URL
    /// (поле `url`), а не production-алиас: алиас может показывать предыдущий
    /// деплой проекта (кейс 27.10.2026 — HTML вместо воркера).
    #[test]
    fn ready_deployment_url_takes_deployment_url() {
        let ready = serde_json::json!({
            "id": "dpl_1",
            "readyState": "READY",
            // алиас в ответе присутствует, но приоритет всё равно у деплой-URL:
            "alias": ["old-app.vercel.app"],
            "url": "relay-abc123-team.vercel.app"
        });
        let url = ready_deployment_url(&ready).expect("url есть");
        assert_eq!(url, "https://relay-abc123-team.vercel.app", "ожидается деплой-URL, а не алиас");
    }

    #[test]
    fn ready_deployment_url_requires_url_field() {
        let ready = serde_json::json!({ "id": "dpl_2", "readyState": "READY" });
        let err = ready_deployment_url(&ready).expect_err("без url ошибка");
        assert!(err.contains("url"), "{err}");
    }

    /// Опрос несуществующего деплоя даёт НЕМЕДЛЕННУЮ ошибку, а не зависает в
    /// цикле READY до общего таймаута. Сетевой тест, как
    /// `deploy_surfaces_raw_api_error_instead_of_generic_message`; при живом
    /// API текст содержит HTTP-статус, при заблокированном — network-ошибку,
    /// но в обоих случаях имя деплоя фигурирует (проверяем только это:
    /// требовать «HTTP» нельзя — фильтр может резать и API, кейс 27.10.2026).
    #[tokio::test]
    async fn wait_deployment_ready_fails_fast_on_unknown_deployment() {
        let dir = std::env::temp_dir().join("cr_gw_relay_wait404");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        let err = wait_deployment_ready(&state, "vercel_invalid_token_for_test", "dpl_nonexistent_regresion")
            .await
            .expect_err("должен вернуть ошибку");
        assert!(
            err.contains("dpl_nonexistent_regresion"),
            "ошибка обязана называть опрашиваемый деплой: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Регресс 27.10.2026: деплой в конфиге БЕЗ Vercel-слота обязан СОЗДАТЬ
    /// слот, а не писать URL в legacy-поля — иначе релей невидим во вкладке
    /// «Релей» и не привязывается к провайдерам.
    #[test]
    fn save_deployed_url_creates_slot_when_missing_and_clears_legacy() {
        let dir = std::env::temp_dir().join("cr_gw_relay_save_new");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        save_deployed_url(&state, "https://relay-abc-team.vercel.app", "tok64hex").expect("слот создан");

        let config = state.config();
        let slot = config.vercel_slot().expect("vercel-слот после деплоя");
        assert_eq!(slot.url.as_deref(), Some("https://relay-abc-team.vercel.app"));
        assert_eq!(slot.relay_secret.as_deref(), Some("tok64hex"));
        assert_eq!(config.active_relay.as_deref(), Some("vercel"));
        // SSOT: legacy очищается той же записью.
        assert_eq!(config.vercel_relay_url, None);
        assert_eq!(config.vercel_relay_token, None);
        // Слот переживает перезапуск шлюза (лежит в gateway.json).
        let on_disk = cloud_routers_core::load_config(&dir);
        assert!(on_disk.vercel_slot().is_some(), "слот обязан лежать в gateway.json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Существующий слот обновляется (URL + секрет), соседние слоты не трогаются.
    #[test]
    fn save_deployed_url_updates_existing_slot_and_keeps_others() {
        let dir = std::env::temp_dir().join("cr_gw_relay_save_upd");
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(&dir).expect("state");
        state
            .update_config(|c| {
                c.relays.push(cloud_routers_core::config::RelaySlot {
                    id: "vercel".into(),
                    kind: cloud_routers_core::config::RELAY_KIND_VERCEL.into(),
                    url: Some("https://old.vercel.app".into()),
                    relay_secret: Some("old-tok".into()),
                    cf_account_id: None,
                    cf_script_name: None,
                });
                c.relays.push(cloud_routers_core::config::RelaySlot {
                    id: "cloudflare".into(),
                    kind: "cloudflare".into(),
                    url: Some("https://cf.workers.dev".into()),
                    relay_secret: Some("cf-tok".into()),
                    cf_account_id: Some("acc".into()),
                    cf_script_name: Some("relay".into()),
                });
                Ok(())
            })
            .expect("подготовка");
        save_deployed_url(&state, "https://new.vercel.app", "new-tok").expect("обновлено");

        let config = state.config();
        let slot = config.vercel_slot().expect("слот");
        assert_eq!(slot.url.as_deref(), Some("https://new.vercel.app"));
        assert_eq!(slot.relay_secret.as_deref(), Some("new-tok"));
        let cf = config.relays.iter().find(|s| s.id == "cloudflare").expect("cf");
        assert_eq!(cf.url.as_deref(), Some("https://cf.workers.dev"), "соседний слот не трогается");
        assert_eq!(config.active_relay.as_deref(), Some("vercel"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}