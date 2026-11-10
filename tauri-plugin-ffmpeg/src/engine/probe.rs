//! ffprobe: JSON-метаданные медиафайла. Полный вывод ffprobe большой
//! (сотни полей на поток) — наружу отдаём компактную проекцию, чтобы не
//! раздувать контекст LLM.

use serde_json::{json, Value};
use std::path::Path;

use super::runner::{run_tool, DEFAULT_RUN_TIMEOUT};

/// Снять метаданные: длительность/размер/битрейт + компактные потоки.
pub fn probe_json(ffprobe: &Path, input: &Path) -> Result<Value, String> {
    let args = vec![
        "-v".to_string(),
        "error".to_string(),
        "-print_format".to_string(),
        "json".to_string(),
        "-show_format".to_string(),
        "-show_streams".to_string(),
        input.to_string_lossy().to_string(),
    ];
    let out = run_tool(ffprobe, &args, DEFAULT_RUN_TIMEOUT)?;
    if !out.success {
        return Err(format!("ffprobe завершился с ошибкой: {}", out.stderr_tail));
    }
    let raw: Value = serde_json::from_str(&out.stdout)
        .map_err(|e| format!("ffprobe вернул не-JSON: {e}"))?;
    Ok(project(&raw))
}

/// Компактная проекция вывода ffprobe: только то, что нужно агенту, чтобы
/// спланировать монтаж (длительность, кодеки, разрешение, fps, аудио).
pub fn project(raw: &Value) -> Value {
    let mut streams = Vec::new();
    if let Some(list) = raw.get("streams").and_then(|v| v.as_array()) {
        for s in list {
            streams.push(json!({
                "index": s.get("index"),
                "codec_type": s.get("codec_type"),
                "codec_name": s.get("codec_name"),
                "width": s.get("width"),
                "height": s.get("height"),
                "r_frame_rate": s.get("r_frame_rate"),
                "duration": s.get("duration"),
                "channels": s.get("channels"),
                "sample_rate": s.get("sample_rate"),
            }));
        }
    }
    json!({
        "streams": streams,
        "format": {
            "format_name": raw.pointer("/format/format_name"),
            "duration": raw.pointer("/format/duration"),
            "size": raw.pointer("/format/size"),
            "bit_rate": raw.pointer("/format/bit_rate"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_keeps_needed_fields_and_drops_noise() {
        let raw = json!({
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264",
                 "width": 1920, "height": 1080, "r_frame_rate": "30/1",
                 "duration": "10.0", "nb_frames": "300", "pix_fmt": "yuv420p"},
                {"index": 1, "codec_type": "audio", "codec_name": "aac",
                 "channels": 2, "sample_rate": "44100", "bits_per_sample": 16}
            ],
            "format": {"format_name": "mov,mp4", "duration": "10.02",
                       "size": "1048576", "bit_rate": "800000", "nb_streams": "2"}
        });
        let p = project(&raw);
        assert_eq!(p["streams"].as_array().unwrap().len(), 2);
        assert_eq!(p["streams"][0]["codec_name"], "h264");
        assert_eq!(p["streams"][0]["width"], 1920);
        assert_eq!(p["streams"][1]["channels"], 2);
        // Шум выкинут:
        assert!(p["streams"][0].get("pix_fmt").is_none());
        assert!(p["streams"][0].get("nb_frames").is_none());
        assert_eq!(p["format"]["duration"], "10.02");
        assert!(p["format"].get("nb_streams").is_none());
    }

    #[test]
    fn project_survives_empty_input() {
        let p = project(&json!({}));
        assert_eq!(p["streams"].as_array().unwrap().len(), 0);
        assert!(p["format"]["duration"].is_null());
    }
}
