//! 🔔 Неблокирующее уведомление об итоге pre-flight VRAM (одна кнопка «ОК»).
//!
//! Плагин не трогает хостовую шину агентов (`event_bus` хоста) — VRAM-канал
//! полностью локальный: `notify_vram` публикует в in-process шину плагина, а
//! `start_forwarding` (вызывается в setup плагина ОДИН раз) форвардит событие
//! на фронтенд как `vram_notice`.

use std::sync::{Arc, Mutex, OnceLock};

/// In-process шина уведомлений VRAM плагина.
#[derive(Clone, Debug)]
pub struct VramNotice {
    pub note: String,
}

type Listener = Arc<dyn Fn(&VramNotice) + Send + Sync>;

#[derive(Default)]
struct VramBus {
    listeners: Mutex<Vec<Listener>>,
}

impl VramBus {
    fn subscribe(&self, listener: Listener) {
        self.listeners.lock().unwrap().push(listener);
    }

    fn publish(&self, event: &VramNotice) {
        for listener in self.listeners.lock().unwrap().iter() {
            listener(event);
        }
    }
}

static BUS: OnceLock<Arc<VramBus>> = OnceLock::new();

fn vram_bus() -> Arc<VramBus> {
    BUS.get_or_init(|| Arc::new(VramBus::default())).clone()
}

/// Отправить юзеру уведомление о VRAM (non-blocking, одна кнопка «ОК»).
pub fn notify_vram(note: String) {
    vram_bus().publish(&VramNotice { note });
}

/// Подписка + форвардинг VramNotice → событие `vram_notice` (UI).
/// Вызывается один раз в setup плагина.
pub fn start_forwarding(app: &tauri::AppHandle) {
    let app = app.clone();
    vram_bus().subscribe(Arc::new(move |notice| {
        use tauri::Emitter;
        let _ = app.emit(
            "vram_notice",
            serde_json::json!({
                "note": notice.note,
            }),
        );
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_publishes_vram_notice_to_bus() {
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let r = received.clone();
        vram_bus().subscribe(Arc::new(move |e| {
            r.lock().unwrap().push(e.note.clone());
        }));
        notify_vram("тест".to_string());
        let got = received.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], "тест");
    }
}