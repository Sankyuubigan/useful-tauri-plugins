//! Ядро плагина `tauri-plugin-cloud-routers`: конфиг, установка, процесс, HTTP-клиент.

pub mod child_env;
pub mod client;
pub mod client_error;
pub mod config;
pub mod gateway_installer;
pub mod gateway_process;
pub mod installer;
pub mod no_login;
pub mod postinstall_contract;

pub mod process;
pub mod reinstall;
#[cfg(test)]
pub mod test_scratch;

pub use client::{ChatMessage, ChatRequest, ComboInfo};
pub use client_error::{CloudStreamError, CLOUD_EMPTY_STREAM_MARKER};
pub use config::{load_config, router_dir, save_config, RouterConfig};
