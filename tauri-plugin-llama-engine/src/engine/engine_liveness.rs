//! Engine liveness smoke (#[ignore]): BeeLlama cuda-13.3 + gemma-4-12B.
//! Запускает LlamaEngine, шлёт «привет», проверяет генерацию токенов.
//! Content может быть пустым (reasoning съедает budget) — принимаем
//! text ИЛИ reasoning в GenerationResult при metrics/stop.
//!
//! Запуск: cargo test -p tauri-plugin-llama-engine engine_liveness -- --ignored --nocapture
//! (только через plugin_test.bat — sccache/MSVC wrapper)

#[cfg(test)]
mod engine_liveness {
    use crate::engine::config::ModelParams;
    use crate::engine::llm_types::{GenerationResult, LlmMessage};
    use crate::engine::LlamaEngine;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    #[ignore]
    fn engine_liveness_hello() {
        let engine_dir = Path::new(r"D:\Programs\llamacpp\backends\beellama\cuda-13.3");
        let model = r"D:\nn\models\llm\uncen\gemma-4-12B\gemma-4-12B-it-qat-q4_0-unquantized-heretic-ja-v2.i1-IQ4_NL.gguf";
        if !engine_dir.join("llama-server.exe").exists() {
            eprintln!("SKIP: no llama-server at {}", engine_dir.display());
            return;
        }
        if !engine_dir.join("cublas64_13.dll").exists() {
            panic!(
                "cublas64_13.dll missing in {} — CUDA runtime not installed",
                engine_dir.display()
            );
        }
        if !Path::new(model).exists() {
            eprintln!("SKIP: no model {}", model);
            return;
        }

        let tokens = Arc::new(AtomicU32::new(0));
        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let engine = {
            let logs = logs.clone();
            let tokens = tokens.clone();
            LlamaEngine::new(
                engine_dir,
                model,
                4096,
                false, // kv keys
                false, // kv values
                0,     // reasoning_budget
                move |m| {
                    logs.lock().unwrap().push(m);
                },
                move |_tok| {
                    tokens.fetch_add(1, Ordering::SeqCst);
                },
            )
        }
        .unwrap_or_else(|e| {
            let tail: Vec<String> = {
                let l = logs.lock().unwrap();
                l.iter().rev().take(30).cloned().collect()
            };
            panic!(
                "LlamaEngine::new failed: {}\n--- log tail ---\n{}",
                e,
                tail.join("\n")
            );
        });

        let msgs = vec![LlmMessage {
            role: "user".into(),
            content: "привет".into(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let params = ModelParams::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let logs2 = logs.clone();

        let result: Result<GenerationResult, String> = engine.generate_chat(
            &msgs,
            128,
            &params,
            "Auto",
            true, // disable_reasoning → prefer content
            cancel,
            "liveness",
            None,
            |_, _| {},
            move |m| {
                logs2.lock().unwrap().push(m);
            },
        );
        drop(engine); // Drop → kill llama-server

        let n = tokens.load(Ordering::SeqCst);
        match result {
            Ok(r) => {
                let text = &r.text;
                let reasoning = &r.reasoning;
                eprintln!(
                    "tokens={} text={:?} reasoning_len={}",
                    n,
                    text.chars().take(200).collect::<String>(),
                    reasoning.len()
                );
                assert!(
                    n >= 1 || !text.trim().is_empty() || !reasoning.trim().is_empty(),
                    "no generation (n={}, text empty, reasoning empty)",
                    n
                );
            }
            Err(e) => {
                let tail: Vec<String> = {
                    let l = logs.lock().unwrap();
                    l.iter().rev().take(30).cloned().collect()
                };
                panic!(
                    "generate failed: {}\n--- log tail ---\n{}",
                    e,
                    tail.join("\n")
                );
            }
        }
    }
}
