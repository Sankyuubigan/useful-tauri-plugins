//! Общий HTTP-слой тула `websearch` — порт `web_http.ts` (King Orch).
//!
//! Перенесено 1-в-1: gzip/deflate/brotli (порт `decodeBody` — декодирование
//! вручную через `flate2`/`brotli` семантикой TS: ошибка декодера = сырые
//! байты; фичи reqwest не годятся — память этой машины не содержит
//! `async-compression`, а crates.io недоступен, см. PRP §4.1), merge
//! Set-Cookie между редиректами, лимит байт на тело ответа, ручные редиректы
//! с классификацией фазы на каждом хопе, человекочитаемые причины сетевых
//! ошибок.
//!
//! Осознанные отличия от TS (см. PRP §3.8 и §4.1), задокументированные здесь:
//! - TLS-профили Node (`minVersion TLSv1.3/TLSv1.2`) заменены повтором попытки
//!   на TLS-фазе (до 3 попыток с паузой 300 мс): reqwest/native-tls на Windows
//!   сам ведёт согласование, а пиновать версии можно только через rustls/ring
//!   (C-компиляция запрещена правилом репозитория). Алгоритмика ретрая
//!   сохранена, профили — нет.
//! - фаза соединения определяется по тексту/типу ошибки reqwest (is_connect,
//!   is_timeout, ключевым словам), а не по событиям сокета Node: точность та же
//!   для DNS/connect/таймаутов, для TLS-обрыва — по ключевым словам handшейка.
//! - `checkUrl` (SSRF-колбэк searxng) не переносится в этом этапе — он
//!   понадобится этапу 2 (PRP §6).
//!
//! Плюс тестовый крюк (документирован в `cli.rs`): env
//! `AGENT_TOOLS_WEBSEARCH_BASE_URL` заменяет origin всех запросов —-live-движки
//! реально не вызываются, `request` идёт на локальный стаб той же структуры.

use std::error::Error as StdError;
use std::io::Read;
use std::str::FromStr;
use std::sync::OnceLock;

use futures_util::StreamExt;
use percent_encoding::AsciiSet;
use regex::Regex;
use reqwest::Client;
use url::Url;

pub const TEST_BASE_URL_ENV: &str = "AGENT_TOOLS_WEBSEARCH_BASE_URL";

/// UA по умолчанию — как в web_http.ts (Chrome 126).
pub const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

const DEFAULT_TIMEOUT_MS: u64 = 20_000;
const DEFAULT_MAX_BYTES: usize = 5 * 1024 * 1024;

/// Фаза соединения, на которой случилась ошибка (используется ретраем TLS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connect,
    Tls,
    Http,
}

/// Ошибка HTTP-слоя с фазой — причина всегда строкой для стека failure'ов.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub message: String,
    pub phase: Phase,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HttpError {}

/// Опции запроса — порт `RequestOptions` TS.
#[derive(Debug, Clone, Default)]
pub struct RequestOpts {
    /// Метод ("GET" | "POST"); дефолт GET.
    pub method: Option<String>,
    /// Дополнительные заголовки; перекрывают дефолтные по имени.
    pub headers: Vec<(String, String)>,
    /// Тело (строка, уже закодированная вызывающим).
    pub body: Option<String>,
    /// Таймаут всего запроса (дефолт 20 000 мс).
    pub timeout_ms: Option<u64>,
    /// Лимит тела ответа (дефолт 5 МБ).
    pub max_bytes: Option<usize>,
    /// Максимум редиректов (дефолт 5).
    pub max_redirects: Option<usize>,
    /// Перекрыть User-Agent.
    pub user_agent: Option<String>,
}

/// Ответ: статус, декодированный текст, финальный URL после редиректов.
/// `url` не читается web_search движками — понадобится этапу 2 (searxng).
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub text: String,
    #[allow(dead_code)]
    pub url: String,
}

fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("клиент reqwest поверх системного TLS всегда строится")
    })
}

/// Тестовый крюк: замена origin всех запросов на значение env (см. cli.rs).
fn rewrite_for_tests(url: &str) -> String {
    let Ok(base) = std::env::var(TEST_BASE_URL_ENV) else {
        return url.to_string();
    };
    let base = base.trim();
    if base.is_empty() {
        return url.to_string();
    }
    let (Ok(base_url), Ok(parsed)) = (Url::parse(base), Url::parse(url)) else {
        return url.to_string();
    };
    let mut rewritten = format!(
        "{}://{}",
        base_url.scheme(),
        base_url.host_str().unwrap_or("127.0.0.1"),
    );
    if let Some(port) = base_url.port() {
        rewritten.push_str(&format!(":{port}"));
    }
    rewritten.push_str("/");
    // Путь+query исходного запроса сохраняются: стаб отвечает по тем же путям.
    changed_path(&parsed, &rewritten)
}

fn changed_path(parsed: &Url, origin: &str) -> String {
    let path = if parsed.path() == "/" || parsed.path().is_empty() {
        String::new()
    } else {
        parsed.path().to_string()
    };
    format!(
        "{origin}{}{}",
        path.trim_start_matches('/'),
        parsed.query().map(|q| format!("?{q}")).unwrap_or_default()
    )
}

/// Классификация ошибки reqwest по фазе — порт `classifyNetworkError`.
fn describe_error(err: &reqwest::Error, hostname: &str) -> HttpError {
    let raw = err.to_string();
    let raw_lower = raw.to_ascii_lowercase();
    // Сеть-слово TLS ищем и в цепочке cause (где reqwest/hyper прячут детали
    // handshake — как в итоговом тексте, так и в источнике).
    let source_text = err
        .source()
        .map(|s| s.to_string().to_ascii_lowercase())
        .unwrap_or_default();
    let tls_words = ["handshake", "certificate", "invalid peer certificate", "tls", "ssl"];
    let tlsish = tls_words.iter().any(|w| source_text.contains(w) || raw_lower.contains(w));

    if err.is_timeout() {
        // TS: таймаут различается по фазе — connect / TLS-рукопожатие / после TLS.
        if tlsish {
            return HttpError {
                message: format!(
                    "Таймаут на TLS-рукопожатии с '{hostname}' — сервер не отвечает на TLS (вероятна региональная блокировка: домен не обслуживает IP из РФ) [raw: {raw}]"
                ),
                phase: Phase::Tls,
            };
        }
        if err.is_connect() {
            return HttpError {
                message: format!("Таймаут соединения (TCP) с '{hostname}' — сервер не отвечает [raw: {raw}]"),
                phase: Phase::Connect,
            };
        }
        return HttpError {
            message: format!(
                "Соединение с '{hostname}' установлено (TLS OK), но сервер не отвечает на запрос (вероятна региональная фильтрация на уровне приложения) [raw: {raw}]"
            ),
            phase: Phase::Http,
        };
    }

    if err.is_connect() {
        let phase = Phase::Connect;
        let dnsish = source_text.contains("dns")
            || source_text.contains("resolve")
            || raw_lower.contains("dns error")
            || raw_lower.contains("resolve");
        if dnsish {
            return HttpError {
                message: format!("Не удалось разрешить DNS для '{hostname}' [raw: {raw}]"),
                phase,
            };
        }
        if raw_lower.contains("refused") {
            return HttpError {
                message: format!("Соединение с '{hostname}' отклонено [raw: {raw}]"),
                phase,
            };
        }
        if raw_lower.contains("unreachable") {
            return HttpError {
                message: format!("Сеть недоступна до '{hostname}' [raw: {raw}]"),
                phase,
            };
        }
        return HttpError {
            message: format!("Соединение с '{hostname}' не установлено [raw: {raw}]"),
            phase,
        };
    }

    // TLS-фаза: handshake-слова в цепочке ошибки reqwest (hyper-tls/native-tls).
    if tls_words.iter().any(|w| raw_lower.contains(w)) {
        return HttpError {
            message: format!(
                "TLS-рукопожатие с '{hostname}' не завершено — соединение могло быть сброшено фильтром [raw: {raw}]"
            ),
            phase: Phase::Tls,
        };
    }

    if raw_lower.contains("connection reset")
        || raw_lower.contains("broken pipe")
        || raw_lower.contains("connection closed")
        || raw_lower.contains("connection aborted")
        || raw_lower.contains("aborted")
    {
        return HttpError {
            message: format!(
                "Соединение с '{hostname}' разорвано до завершения ответа [raw: {raw}]"
            ),
            phase: Phase::Http,
        };
    }

    HttpError {
        message: raw,
        phase: Phase::Http,
    }
}

/// HTTP-запрос с ручными редиректами и сбором куков — порт `request()`.
pub async fn request(url: &str, opts: &RequestOpts) -> Result<HttpResponse, HttpError> {
    let method = opts.method.clone().unwrap_or_else(|| "GET".into());
    let timeout = std::time::Duration::from_millis(opts.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let max_bytes = opts.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let max_redirects = opts.max_redirects.unwrap_or(5);
        let mut cookie_jar: Vec<(String, String)> = Vec::new();

    let mut current = rewrite_for_tests(url);
    let mut hop = 0usize;
    loop {
        let parsed = match Url::parse(&current) {
            Ok(u) => u,
            Err(e) => {
                return Err(HttpError {
                    message: format!("неверный URL '{current}': {e}"),
                    phase: Phase::Http,
                })
            }
        };
        let hostname = parsed
            .host_str()
            .unwrap_or_default()
            .to_string();

        let mut request = client()
            .request(
                reqwest::Method::from_str(&method)
                    .unwrap_or(reqwest::Method::GET),
                current.as_str(),
            )
            .timeout(timeout)
            .header("User-Agent", opts.user_agent.clone().unwrap_or_else(|| DEFAULT_UA.to_string()))
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Encoding", "gzip, deflate, br")
            .header("Accept-Language", "ru,en;q=0.8");
        if !cookie_jar.is_empty() {
            let header = cookie_jar
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
                .join("; ");
            request = request.header("Cookie", header);
        }
        for (name, value) in &opts.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if let Some(body) = &opts.body {
            request = request.body(body.clone());
        }

        // Повтор на TLS-обрыве: до 3 попыток (port алгоритмики TLS-профилей TS).
        let mut response = None;
        let mut last_err: Option<HttpError> = None;
        for attempt in 0..3usize {
            match request
                .try_clone()
                .expect("запрос клонируется: тело — строка")
                .send()
                .await
            {
                Ok(resp) => {
                    response = Some(resp);
                    break;
                }
                Err(err) => {
                    let described = describe_error(&err, &hostname);
                    // Ретраится только TLS-фаза (как в TS — там только профили TLS).
                    let retryable = described.phase == Phase::Tls && attempt + 1 < 3;
                    last_err = Some(described);
                    if !retryable {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            }
        }
        let resp = match response {
            Some(r) => r,
            None => {
                return Err(last_err.unwrap_or(HttpError {
                    message: "запрос не выполнен".into(),
                    phase: Phase::Http,
                }))
            }
        };

        // merge Set-Cookie между редиректами: только пары name=value (как в TS).
        let set_cookie: Vec<String> = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(str::to_string)
            .collect();
        for cookie in &set_cookie {
            let pair = cookie.split(';').next().unwrap_or_default();
            if let Some((name, value)) = pair.split_once('=') {
                if !name.trim().is_empty() {
                    let (name, value) = (name.trim().to_string(), value.trim().to_string());
                    // Замена существующего name сохраняет порядок вставки (как Map в TS).
                    if let Some(slot) = cookie_jar.iter_mut().find(|(n, _)| *n == name) {
                        slot.1 = value;
                    } else {
                        cookie_jar.push((name, value));
                    }
                }
            }
        }

        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        if (300..400).contains(&status) && hop < max_redirects {
            if let Some(location) = location {
                // new URL(location, current) — относительная склейка как в TS.
                current = match Url::parse(&current).and_then(|base| base.join(&location)) {
                    Ok(u) => rewrite_for_tests(u.as_str()),
                    Err(e) => {
                        return Err(HttpError {
                            message: format!("битый редирект '{location}': {e}"),
                            phase: Phase::Http,
                        })
                    }
                };
                hop += 1;
                continue;
            }
        }

        // Лимит байт на тело ответа (как в TS: destroy при превышении),
        // декодирование — ПОСЛЕ лимита, по сырому размеру (семантика TS).
        let content_encoding = resp
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        let mut stream = resp.bytes_stream();
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    return Err(HttpError {
                        message: format!("Ошибка чтения ответа от '{hostname}': {e}"),
                        phase: Phase::Http,
                    })
                }
            };
            if body.len() + chunk.len() > max_bytes {
                return Err(HttpError {
                    message: format!("Ответ больше лимита {max_bytes} байт"),
                    phase: Phase::Http,
                });
            }
            body.extend_from_slice(&chunk);
        }
        drop(stream);

        let decoded = decode_body(body, &content_encoding);
        let text = String::from_utf8_lossy(&decoded).into_owned();
        return Ok(HttpResponse {
            status,
            text,
            url: current,
        });
    }
}

// ─────────────────────────── Утилиты парсинга HTML ───────────────────────────

/// Порт `decodeBody`: по заголовку content-encoding. Ошибка любого декодера
/// (или неизвестная кодировка) → сырые байты, как в TS (все try/catch).
fn decode_body(buf: Vec<u8>, encoding: &str) -> Vec<u8> {
    // Как в TS (web_http.ts:15): значение заголовка — toLowerCase().trim().
    let encoding = encoding.trim().to_lowercase();
    match encoding.as_str() {
        "gzip" => {
            let mut out = Vec::new();
            match flate2::read::MultiGzDecoder::new(buf.as_slice()).read_to_end(&mut out) {
                Ok(_) => out,
                Err(_) => buf,
            }
        }
        "deflate" => {
            // zlib.inflateSync — zlib-обёртка (header 0x78..), не raw inflate.
            let mut out = Vec::new();
            match flate2::read::ZlibDecoder::new(buf.as_slice()).read_to_end(&mut out) {
                Ok(_) => out,
                Err(_) => buf,
            }
        }
        "br" => {
            let mut out = Vec::new();
            match brotli::Decompressor::new(buf.as_slice(), 4096).read_to_end(&mut out) {
                Ok(_) => out,
                Err(_) => buf,
            }
        }
        _ => buf,
    }
}

/// Порт `unescapeHtml` (порядок замен сохранён, включая его «двойное
/// разъюздирование» амперсанда — поведение 1-в-1 с TS).
pub fn unescape_html(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&hellip;", "…")
}

/// Порт `stripTags`: вырезать теги → unescape → схлопнуть пробелы → trim.
pub fn strip_tags(input: &str) -> String {
    let tags = Regex::new(r"<[^>]+>").expect("константная регулярка");
    let text = tags.replace_all(input, " ").into_owned();
    let spaces = Regex::new(r"\s+").expect("константная регулярка");
    spaces.replace_all(&unescape_html(&text), " ").trim().to_string()
}

// ── findBalancedBlock / parseBlocksByClass ───────────────────────────────────

/// Найти сбалансированный блок `<tag ...>...</tag>` начиная с позиции.
/// Порт `findBalancedBlock` (учёт self-closing и вложенности).
pub fn find_balanced_block(html: &str, tag: &str, start_idx: usize) -> Option<(usize, usize)> {
    if start_idx > html.len() {
        return None;
    }
    let open_re = Regex::new(&format!(r"(?i)<{}\b[^>]*>", regex::escape(tag)))
        .expect("регулярка из проверенного тега");
    let open_match = open_re.find(&html[start_idx..])?;
    let block_start = start_idx + open_match.start();
    let content_start = block_start + open_match.end();

    let token_re = Regex::new(&format!(
        r"(?i)<(\/?)\s*{}\b([^>]*?)(\/?)>",
        regex::escape(tag)
    ))
    .expect("регулярка из проверенного тега");

    let rest = &html[content_start..];
    let mut depth = 1usize;
    let mut end_abs = None;
    for caps in token_re.captures_iter(rest) {
        let whole = caps.get(0).expect("captives has 0");
        let is_close = caps.get(1).map(|c| c.as_str()).unwrap_or_default() == "/";
        let is_self_close = caps.get(3).map(|c| c.as_str()).unwrap_or_default() == "/";
        if !is_close && !is_self_close {
            depth += 1;
            continue;
        }
        if is_close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                end_abs = Some(content_start + whole.end());
                break;
            }
        }
    }
    end_abs.map(|end| (block_start, end))
}

/// Разрезать HTML на сбалансированные блоки с классом — порт `parseBlocksByClass`.
pub fn parse_blocks_by_class(html: &str, class_name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let re_str = format!(
        r#"(?i)<([a-z][a-z0-9]*)\b[^>]*class="[^"]*\b{}\b[^"]*"[^>]*>"#,
        regex::escape(class_name)
    );
    let Ok(re) = Regex::new(&re_str) else {
        return out;
    };
    // JS крутит один regex с lastIndex; эквивалент: после каждого найденного
    // блока продолжаем с его конца (find_at с курсором).
    // regex crate не имеет find_at: режем вручную через Captures поверх среза
    // и сдвигаем логику сами (эквивалент re.lastIndex = block.end).
    let mut cursor = 0usize;
    loop {
        if cursor >= html.len() {
            break;
        }
        let slice = &html[cursor..];
        let Some(m) = re.captures(slice) else { break };
        let whole = m.get(0).expect("captures 0");
        let tag = m.get(1).map(|c| c.as_str()).unwrap_or_default().to_string();
        let match_abs = cursor + whole.start();
        match find_balanced_block(html, &tag, match_abs) {
            Some((_start, end)) => {
                out.push(html[match_abs..end].to_string());
                cursor = end;
            }
            // В TS блок без пары пропускается, и regex идёт дальше по всей строке.
            None => cursor += whole.end().max(1),
        }
    }
    out
}

/// Первый http(s)-href из блока — порт `firstHref`.
pub fn first_href(block: &str) -> Option<String> {
    let re = Regex::new(r#"(?i)href="(https?://[^"]+)""#).expect("константная регулярка");
    re.captures(block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Нормализация http(s) URL — порт `normalizeHttpUrl` (hash отрезается).
pub fn normalize_http_url(raw: &str, base: Option<&str>) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let raw = if let Some(stripped) = raw.strip_prefix("//") {
        format!("https:{stripped}")
    } else {
        raw.to_string()
    };
    let parsed = match Url::parse(&raw) {
        Ok(u) => u,
        Err(_) => {
            // Относительный URL: склейка с base (если он есть), как new URL(raw, base).
            let base = base?;
            let base_url = Url::parse(base).ok()?;
            base_url.join(&raw).ok()?
        }
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    // JS: u.hash = '' — фрагмент отрезается.
    let mut u = parsed;
    u.set_fragment(None);
    Some(u.to_string())
}

// ───────────────────── Кодирование, как в браузерах/Node ─────────────────────

/// `encodeURIComponent` из JS: всё, кроме A-Za-z0-9 - _ . ! ~ * ' ( ).
///
/// NON_ALPHANUMERIC кодирует всё, кроме букв/цифр, — возвращаем белый список JS.
const JS_COMPONENT_SET: &AsciiSet = &{
    use percent_encoding::NON_ALPHANUMERIC;
    NON_ALPHANUMERIC
        .remove(b'-').remove(b'_').remove(b'.').remove(b'!')
        .remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')')
};

/// Порт `encodeURIComponent(query)` для URL-интерполяции движков.
pub fn js_encode_uri_component(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, JS_COMPONENT_SET).to_string()
}

/// Порт `new URLSearchParams(obj).toString()` для POST-тел: space → '+'.
pub fn form_urlencoded(pairs: &[(&str, &str)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn decode_body_handls_gzip_deflate_brotli_and_falls_back_to_raw() {
        let plain = "привет, поисковик".as_bytes().to_vec();

        // gzip: сжатие MultiGzEncoder → decode раскрывает.
        let mut gz = Vec::new();
        {
            let mut enc =
                flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
            enc.write_all(&plain).expect("gzip encoder");
            enc.finish().expect("gzip finish");
        }
        assert_eq!(decode_body(gz.clone(), "gzip"), plain);
        assert_eq!(decode_body(gz.clone(), "GZIP"), plain, "заголовок может прийти в верхнем регистре");

        // deflate: zlib-обёртка (inflateSync), не raw.
        let mut zl = Vec::new();
        {
            let mut enc =
                flate2::write::ZlibEncoder::new(&mut zl, flate2::Compression::default());
            enc.write_all(&plain).expect("zlib encoder");
            enc.finish().expect("zlib finish");
        }
        assert_eq!(decode_body(zl, "deflate"), plain);

        // brotli.
        let mut br = Vec::new();
        {
            let mut enc =
                brotli::CompressorWriter::new(&mut br, 4096, 9, 22);
            enc.write_all(&plain).expect("brotli encoder");
            enc.flush().expect("brotli flush");
        }
        assert_eq!(decode_body(br, "br"), plain);

        // Битые payload'ы — TS возвращает сырые байты без ошибки.
        let broken_gzip = vec![0x1f, 0x8b, 0xff, 0x00, 0x01, 0x02];
        assert_eq!(decode_body(broken_gzip.clone(), "gzip"), broken_gzip);
        let broken_br = vec![0x00, 0xff, 0x21];
        assert_eq!(decode_body(broken_br.clone(), "br"), broken_br);
        assert_eq!(decode_body(plain.clone(), ""), plain);
        assert_eq!(decode_body(plain.clone(), "identity"), plain);
    }

    #[test]
    fn unescape_html_follows_ts_replacement_order() {
        assert_eq!(unescape_html("a &amp; b"), "a & b");
        // TS: {amp} → & идёт ПЕРВЫМ шагом, поэтому итог «&lt;» → остаётся «<».
        assert_eq!(unescape_html("&amp;lt;"), "<", "двойное разъюздирование как в TS");
        assert_eq!(unescape_html("x&#x27;y&quot;z&nbsp;…w"), "x'y\"z …w");
    }

    #[test]
    fn strip_tags_collapses_whitespace() {
        assert_eq!(strip_tags("<b>Hello</b>  <i>world</i>"), "Hello world");
        assert_eq!(strip_tags("a<b>c</b>d"), "a c d");
        assert_eq!(strip_tags(""), "");
    }

    #[test]
    fn balanced_block_walks_nested_tags() {
        let html = r#"<div class="result"><a href="u1">t</a><div class="snippet">s</div></div>tail"#;
        let (start, end) = find_balanced_block(html, "div", 0).expect("блок найден");
        assert_eq!(&html[start..end], r#"<div class="result"><a href="u1">t</a><div class="snippet">s</div></div>"#);
    }

    #[test]
    fn parse_blocks_by_class_splits_all_results() {
        let html = concat!(
            r#"<div class="result"><a href="u1">t1</a></div>"#,
            r#"<div class="result"><a href="u2">t2</a></div>"#,
        );
        let blocks = parse_blocks_by_class(html, "result");
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("u1"));
        assert!(blocks[1].contains("u2"));
    }

    #[test]
    fn parse_blocks_skips_unclosed_block_without_loop() {
        let html = r#"<span class="tlink">без закрытия"#;
        assert!(parse_blocks_by_class(html, "tlink").is_empty());
    }

    #[test]
    fn first_href_extracts_first_http_url() {
        assert_eq!(
            first_href(r#"<a href="https://example.com/a">x</a>"#),
            Some("https://example.com/a".to_string())
        );
        assert_eq!(first_href(r#"<a href="/relative">x</a>"#), None);
    }

    #[test]
    fn normalize_url_drops_fragments_and_junk_schemes() {
        assert_eq!(
            normalize_http_url("https://e.test/p#frag", None),
            Some("https://e.test/p".to_string())
        );
        assert_eq!(normalize_http_url("javascript:alert(1)", None), None);
        assert_eq!(
            normalize_http_url("/p", Some("https://e.test/base/")),
            Some("https://e.test/p".to_string())
        );
        assert_eq!(
            normalize_http_url("//e.test/p", None),
            Some("https://e.test/p".to_string())
        );
    }

    #[test]
    fn js_encode_matches_encode_uri_component() {
        assert_eq!(js_encode_uri_component("a b"), "a%20b");
        assert_eq!(js_encode_uri_component("купить/перчатки"), "%D0%BA%D1%83%D0%BF%D0%B8%D1%82%D1%8C%2F%D0%BF%D0%B5%D1%80%D1%87%D0%B0%D1%82%D0%BA%D0%B8");
        // '+' в encodeURIComponent кодируется (в отличие от form-urlencoded).
        assert_eq!(js_encode_uri_component("a+b-c.d!e~f*g'h(i)j"), "a%2Bb-c.d!e~f*g'h(i)j");
    }

    #[test]
    fn form_urlencoded_uses_plus_for_spaces() {
        assert_eq!(
            form_urlencoded(&[("query", "rust tokio"), ("cat", "web")]),
            "query=rust+tokio&cat=web"
        );
    }
}
