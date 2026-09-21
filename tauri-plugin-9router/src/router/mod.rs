//! Ядро плагина `tauri-plugin-9router`: конфиг, установка, процесс, HTTP-клиент.

pub mod client;
pub mod config;
pub mod installer;
pub mod process;

pub use client::{ChatMessage, ChatRequest, ComboInfo};
pub use config::{load_config, router_dir, save_config, NineRouterConfig};
