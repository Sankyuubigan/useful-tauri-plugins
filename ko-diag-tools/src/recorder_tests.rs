//! Тесты рекордера — вынесены из recorder.rs (гейт «файлы ≤ 500 строк» §8 PRP,
//! прецедент: `video_tests.rs` хоста). Являются `mod tests` через `#[path]`,
//! поэтому видят приватную поверхность через `super::*`.

use super::*;
use std::time::SystemTime;

/// Тестовый каталог: уникальный, чтобы параллельные прогоны не
/// пересекались (секция §1.2: sandbox `ko_diag_tools_*`).
fn sandbox(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    std::env::temp_dir().join(format!("ko_diag_tools_{tag}_{nanos}"))
}

/// Тестовый конфиг: интервал ровно на минимуме sysinfo (200 мс) —
/// статистика ЦП реально обновляется, тики идут честно.
fn test_cfg() -> RecorderConfig {
    RecorderConfig {
        interval: Duration::from_millis(200),
        max_duration: Duration::from_secs(5),
        max_file_bytes: 1 << 20,
        tail_lines: 40,
    }
}

#[test]
fn interval_below_minimum_is_honestly_rejected() {
    let dir = sandbox("min_interval");
    std::fs::create_dir_all(&dir).unwrap();
    // match вместо expect_err: RecorderControl не Debug (JoinHandle) —
    // и не должен им быть: живущий поток не обязан печататься.
    let result = start_recording(
        &dir,
        RecorderConfig {
            interval: Duration::from_millis(30),
            ..test_cfg()
        },
    );
    let err = match result {
        Ok(_) => panic!("интервал 30 мс обязан дать отказ, а не запуститься"),
        Err(e) => e,
    };
    assert!(err.contains("200"), "причина обязана называть минимум: {err}");
}

#[test]
fn ticks_are_written_and_stopped() {
    let dir = sandbox("ticks");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = test_cfg();
    let control = start_recording(&dir, cfg.clone()).expect("рекордер запускается");
    // Даём потоку 3-4 тика (3 × 200 мс) + запас на НОЛЬ-тик при CI-е.
    thread::sleep(Duration::from_millis(900));
    let summary = stop_recording(control, &cfg);
    assert!(summary.ticks >= 1, "должен быть хотя бы один тик: {}", summary.ticks);
    assert!(!summary.tail.is_empty(), "хвост обязан быть");
    assert!(
        summary.path.exists(),
        "лог обязан быть на диске: {}",
        summary.path.display()
    );
    let content = std::fs::read_to_string(&summary.path).unwrap();
    assert!(
        content.contains("РЕКОРДЕР ОСТАНОВЛЕН"),
        "финальная строка обязана быть:\n{content}"
    );
    assert!(
        content.contains("=== ДИАГНОСТИЧЕСКИЙ РЕКОРДЕР"),
        "шапка обязана быть:\n{content}"
    );
    assert!(
        content.contains("ЦП "),
        "типовой тик содержит ЦП:\n{content}"
    );
}

#[test]
fn latest_log_finds_file_or_none() {
    let dir = sandbox("latest");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(latest_log(&dir), None, "пустой каталог → None");
    let cfg = test_cfg();
    let control = start_recording(&dir, cfg.clone()).expect("старт ок");
    thread::sleep(Duration::from_millis(500));
    stop_recording(control, &cfg);
    let found = latest_log(&dir).expect("файл рекордера обязан найтись");
    assert!(found.starts_with(&dir), "найден в каталоге: {}", found.display());
}

#[test]
fn next_recorder_file_is_monotonic() {
    let dir = sandbox("monotonic");
    std::fs::create_dir_all(&dir).unwrap();
    let first = next_recorder_file(&dir).unwrap();
    assert!(first.ends_with("diag_recorder_1.log"), "первый = _1: {}", first.display());
    std::fs::write(&first, "tick").unwrap();
    let second = next_recorder_file(&dir).unwrap();
    assert!(second.ends_with("diag_recorder_2.log"), "второй = _2: {}", second.display());
}

#[test]
fn read_log_tail_happy_and_missing() {
    let dir = sandbox("tail");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("не_рекордер.log");
    std::fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();
    let tail = read_log_tail(&file, 2);
    assert_eq!(tail, vec!["d".to_string(), "e".to_string()], "хвост — 2 последние строки: {tail:?}");
    // Больше строк, чем есть → весь файл.
    assert_eq!(read_log_tail(&file, 50).len(), 5);
    // Нет файла → пустой хвост (не паника): причина окончит fs-доступ.
    assert!(read_log_tail(&dir.join("нет.log"), 10).is_empty());
}
