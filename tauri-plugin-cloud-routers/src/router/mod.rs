//! Ядро плагина `tauri-plugin-cloud-routers`: конфиг, установка, процесс, HTTP-клиент.

pub mod child_env;
pub mod client;
pub mod config;
pub mod gateway_installer;
pub mod gateway_process;
pub mod installer;
pub mod postinstall_contract;

pub mod process;
#[cfg(test)]
pub mod test_scratch;

pub use client::{ChatMessage, ChatRequest, ComboInfo};
pub use config::{load_config, router_dir, save_config, RouterConfig};
