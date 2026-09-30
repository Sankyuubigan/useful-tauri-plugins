//! Диагностика шага скачивания установщика релиза (откат версии).
//!
//! Тест НЕ вызывает `install_release` — та завершает приложение (`app.exit(0)`),
//! чтобы освободить exe под замену инсталлером.
//! Проверяется ровно тот шаг, где откат падал у пользователя: разбор URL,
//! скачивание ассета GitHub Releases через 6-уровневую цепочку движка и
//! получение валидного .exe на диске.
//!
//! Запуск (правило AGENTS.md §8 — только через test.bat):
//!   `cargo test --test rollback_download -- --ignored --nocapture`
//!
//! `#[ignore]`: требует сети, качает ~14 МБ, поэтому не входит в обычный прогон.

use std::time::Duration;

/// Реальный ассет публичного релиза (проверен через GitHub Releases API).
const ASSET_URL: &str = "https://github.com/Sankyuubigan/king_orch/releases/download/v26.9.177/King.Orch_26.9.177_x64-setup.exe";
const EXPECTED_NAME: &str = "King.Orch_26.9.177_x64-setup.exe";

#[test]
fn installer_file_name_is_parsed_from_url() {
    let (name, path) =
        tauri_plugin_about_updates::installer::installer_path(ASSET_URL).expect("URL корректен");
    assert_eq!(name, EXPECTED_NAME);
    assert!(path.ends_with(EXPECTED_NAME));
}

/// `flavor = "multi_thread"` обязателен: уровень 2 движка скачивания использует
/// `tokio::task::block_in_place`, который паникует на current_thread-рантайме.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "требует сети и качает ~14 МБ; запуск: cargo test -- --ignored --nocapture"]
async fn downloads_real_release_asset() {
    let started = std::time::Instant::now();
    let (_name, path) = tauri_plugin_about_updates::installer::download_installer(ASSET_URL)
        .await
        .unwrap_or_else(|e| panic!("скачивание установщика провалилось: {}", e));

    let meta = std::fs::metadata(&path)
        .unwrap_or_else(|e| panic!("скачанный файл недоступен {}: {}", path.display(), e));
    assert!(
        meta.len() > 10 * 1024 * 1024,
        "установщик должен быть больше 10 МБ, получено {} байт",
        meta.len()
    );

    // PE-сигнатура: MZ — установщик, а не HTML-заглушка/ошибка прокси.
    let mut head = [0u8; 2];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(&path).expect("открыть установщик");
        f.read_exact(&mut head).expect("прочитать сигнатуру");
    }
    assert_eq!(&head, b"MZ", "файл не является Windows-исполняемым");

    let _ = std::fs::remove_file(&path);
    println!(
        "OK: {} байт за {:?}",
        meta.len(),
        Duration::from_secs_f32(started.elapsed().as_secs_f32())
    );
}
