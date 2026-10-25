//! Ядро плагина `tauri-plugin-cloud-routers`: конфиг, установка, процесс, HTTP-клиент.

pub mod child_env;
pub mod client;
pub mod client_error;
pub mod combos;
pub mod config;
pub mod gateway_events;
pub mod gateway_installer;
pub mod gateway_process;
pub mod gateway_supervisor;
pub mod installer;
pub mod no_login;
pub mod postinstall_contract;

pub mod process;
pub mod reinstall;
pub mod sse;
#[cfg(test)]
pub mod test_scratch;

pub use client::{ChatMessage, ChatRequest};
pub use combos::{ComboInfo, get_combos, get_models, is_healthy};
pub use client_error::{CloudStreamError, CLOUD_EMPTY_STREAM_MARKER};
pub use config::{load_config, router_dir, save_config, RouterConfig};
