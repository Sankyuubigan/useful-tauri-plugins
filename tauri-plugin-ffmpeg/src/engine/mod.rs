//! Engine-модуль плагина: установка (installer), запуск subprocess (runner),
//! метаданные через ffprobe (probe). Публичный API для нативных тулов хоста —
//! функции принимают каталоги параметрами, без продуктовых имён.

pub mod installer;
pub mod probe;
pub mod runner;

pub use installer::{
    ensure_ffmpeg, ffmpeg_dir, ffmpeg_exe, ffprobe_exe, is_installed, tool_version,
    FFMPEG_DIR_NAME,
};
pub use probe::probe_json;
pub use runner::{run_tool, RunOutput, DEFAULT_RUN_TIMEOUT};
