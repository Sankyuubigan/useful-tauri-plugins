use tauri::AppHandle;

/// Совместимая обёртка для старого кода: маршрутизирует через единый
/// кастомный `log::Log` логгер хоста (core rules §2.5). Таймстемп, stderr,
/// вкладка «Логи» и файл `test/last_logs` — единая точка в host-логгере,
/// поэтому здесь НЕТ дублирующей записи/emit.
pub fn app_log<R: tauri::Runtime>(_app: &AppHandle<R>, msg: &str) {
    log::info!("{msg}");
}