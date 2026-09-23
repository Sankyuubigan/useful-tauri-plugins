//! Движок llama.cpp — инференс через ОТДЕЛЬНЫЙ процесс `llama-server.exe`
//! (полный релиз llama.cpp), общение по HTTP localhost (random-порт + api-key).
//! Приложение НЕ линкует llama.cpp нативно (см. global_ai_docs/llama_cpp_engine.md).
//!
//! Модуль — переиспользуемое ядро плагина `tauri-plugin-llama-engine`. Хост
//! пере-экспортирует его через собственный фасад `crate::infra::*` (см. README).

pub mod analytics;
pub mod config;
pub mod detokenizer;
pub mod downloader;
pub mod gpu_detector;
pub mod llamacpp_installer;
pub mod llm;
pub mod llm_gguf;
pub mod llm_multimodal;
pub mod llm_types;
pub mod mem_profiler;
pub mod mmproj;
pub mod process_util;
pub mod vram;
pub mod vram_estimate;

// download_fallback удалён: единый движок скачивания — tauri-plugin-downloader (SSOT).

// ── Re-exports (совместимость с прежним фасадом хоста `crate::infra::*`) ──

pub use config::{
    auto_detect_mmproj, find_catalog_entry_for_model, find_sibling_llm_for_mmproj, is_mmproj_file,
    load_catalog, load_config, load_config_early, save_config, CatalogEntry, EngineConfig,
    ModelMeta, ModelParams,
};
pub use llm::{
    build_json_object_grammar_with_keys, build_json_only_grammar, chain_err, estimate_vram_mb,
    extract_f32_from_gguf, extract_gguf_arch, extract_model_filename, extract_u32_from_gguf,
    extract_u32_with_arch, get_hybrid_grammar, llm_history, message_phase, push_report,
    ChatAttachment, ChatMessage, GrammarSpec, LlamaEngine, LlmMessage, PromptFormat, SubCall,
    ToolCallInfo, ToolDefinition, FunctionDef, ToolCall,
};
pub use llm_gguf::{extract_i64_array_from_gguf, extract_i64_array_with_arch};
pub use mem_profiler::{current_process_rss, peak_line, MemGuard, MemSampler};
pub use mmproj::ensure_mmproj_for_model;
pub use vram::notify_vram;