//! CSDN — порт `searchCsdn` из web_search.ts: JSON API so.csdn.net (UA Apifox).

use super::super::http;
use super::{http_status_error, SearchResult};

pub(super) async fn search(query: &str, _limit: usize) -> Result<Vec<SearchResult>, String> {
    let url = format!(
        "https://so.csdn.net/api/v3/search?q={}&t=all&p=1&s=0&tm=0&lv=-1&ft=0&ct=-1&pnt=-1&ry=-1&ss=-1&dct=-1&vt=-1&dms=-1&vip=-1&hit=1&ec=1&c_p=1&c_r=1&showSrc=1&showBlog=1",
        http::js_encode_uri_component(query)
    );
    let opts = http::RequestOpts {
        headers: vec![
            ("User-Agent".into(), "Apifox/1.0.0 (https://apifox.com)".into()),
            ("Host".into(), "so.csdn.net".into()),
            ("Accept".into(), "application/json".into()),
        ],
        timeout_ms: Some(4000),
        ..Default::default()
    };
    let res = http::request(&url, &opts).await.map_err(|e| e.message)?;
    if res.status != 200 {
        return Err(http_status_error(res.status));
    }
    let json: serde_json::Value =
        serde_json::from_str(&res.text).map_err(|_| "невалидный JSON".to_string())?;

    let mut out = Vec::new();
    for r in json.get("result_vos").and_then(|d| d.as_array()).into_iter().flatten() {
        let title = http::strip_tags(&str_field(r, "title"));
        let url2 = crate::tools::websearch::engines::clean_tracking_url(
            &http::normalize_http_url(&str_field(r, "url_location"), Some("https://so.csdn.net/"))
                .unwrap_or_default(),
        );
        if title.is_empty() || url2.is_empty() {
            continue;
        }
        out.push(SearchResult {
            title: crate::tools::websearch::engines::brave::clip_chars(&title, 200),
            url: url2,
            snippet: crate::tools::websearch::engines::brave::clip_chars(&http::strip_tags(&str_field(r, "digest")), 400),
            source: {
                let nick = str_field(r, "nickname");
                if nick.is_empty() { "csdn.net".into() } else { nick }
            },
        });
    }
    Ok(out)
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(text: &str) -> Result<Vec<SearchResult>, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|_| "невалидный JSON".to_string())?;
        let mut out = Vec::new();
        for r in json.get("result_vos").and_then(|d| d.as_array()).into_iter().flatten() {
            let title = http::strip_tags(&str_field(r, "title"));
            let url2 = crate::tools::websearch::engines::clean_tracking_url(
                &http::normalize_http_url(&str_field(r, "url_location"), Some("https://so.csdn.net/")).unwrap_or_default(),
            );
            if title.is_empty() || url2.is_empty() { continue; }
            out.push(SearchResult {
                title: crate::tools::websearch::engines::brave::clip_chars(&title, 200),
                url: url2,
                snippet: crate::tools::websearch::engines::brave::clip_chars(&http::strip_tags(&str_field(r, "digest")), 400),
                source: {
                    let nick = str_field(r, "nickname");
                    if nick.is_empty() { "csdn.net".into() } else { nick }
                },
            });
        }
        Ok(out)
    }

    #[test]
    fn fixture_strips_html_cleans_tracking_and_defaults_source() {
        let text = r#"{"result_vos":[
            {"title": "<b>rtos</b> guide", "url_location": "https://b.csdn.net/rtos?ops_request_misc=x&utm_source=y&id=7",
             "digest": "короткая выжимка", "nickname": "some_one"},
            {"title": "no url", "url_location": ""},
            {"title": "clean", "url_location": "https://clean.t/p?a=b"}
        ]}"#;
        let out = parse_json(text).expect("парсится");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "rtos guide");
        assert_eq!(out[0].url, "https://b.csdn.net/rtos?id=7");
        assert_eq!(out[0].snippet, "короткая выжимка");
        assert_eq!(out[0].source, "some_one");
        assert_eq!(out[1].source, "csdn.net");
    }
}
