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

// ─── Прогноз ≠ факт (G4, tasks/31.10.26) ───
// Два РАЗНЫХ состояния, смешивание которых заставило окно врать юзеру
// («Не хватает видеопамяти» по оценке до запуска, при том что модель
// полностью загрузилась). Формулировка «не хватает» разрешена ТОЛЬКО
// в факт-сообщении, источник которого — отказ аллокации от самого движка.

/// Прогноз: pre-flight оценил потребность ДО запуска и урезал оффлоад.
/// Здесь нельзя утверждать «не хватает» — оценка бывала завышена
/// (юзер 8 ГБ, 30.10.26: «Прирост 4899 МБ» при 7700 свободных).
pub fn vram_forecast_notice(note: &str) -> String {
    format!(
        "⚙️ По оценке до запуска VRAM может не хватить на полный оффлоад — оффлоад урезан заранее, чтобы модель запустилась.\n\n{}",
        note
    )
}

/// Факт: движок сам сообщил отказ аллокации (OOM-тексты в stderr/диагнозах).
/// Единственное место, где честно звучит «не хватило»: оно подкреплено
/// текстом отказа. `what` — что не смогло выделиться, `action` — что сделано.
pub fn vram_oom_fact_notice(what: &str, action: &str) -> String {
    format!("⚠️ Движку не хватило памяти: {}. {}", what, action)
}

/// Прогноз нехватки ОЗУ (host RAM) до запуска — для pinned-буферов BeeLlama,
/// потребность которых не выводится точной формулой (замеры 30.10.26 дают
/// 236-277 КиБ/токен при одном ctx). Правило то же: прогноз не утверждает факт.
pub fn ram_forecast_notice(note: &str) -> String {
    format!("⚙️ По оценке до запуска движку может не хватить обычной памяти (ОЗУ).\n\n{}", note)
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

    /// G4: прогноз НЕ утверждает «не хватает» — только оценку и действие.
    #[test]
    fn forecast_notice_never_claims_shortage_as_fact() {
        let n = vram_forecast_notice("Pre-flight VRAM: модели по оценке нужно ~8603 МБ");
        assert!(!n.contains("Не хватает"), "прогноз не имеет права говорить «Не хватает»: {}", n);
        assert!(!n.contains("не хватило"), "прогноз не имеет права говорить «не хватило»: {}", n);
        assert!(n.contains("По оценке"), "прогноз обязан помечать себя оценкой: {}", n);
        assert!(n.contains("урезан"), "прогноз обязан сообщать действие: {}", n);
    }

    /// G4: факт-формулировка «не хватило» допустима ТОЛЬКО в факт-сообщении,
    /// построенном из отказа аллокации движка.
    #[test]
    fn fact_notice_states_shortage_from_engine_report() {
        let n = vram_oom_fact_notice(
            "не смогла выделиться область CUDA_Host 4.5 ГБ (свободно 10.2 ГБ)",
            "Перезапускаю движок с меньшим оффлоадом.",
        );
        assert!(n.contains("не хватило памяти"), "факт обязан говорить прямо: {}", n);
        assert!(n.contains("CUDA_Host"), "факт обязан цитировать отказ движка: {}", n);
        assert!(n.contains("Перезапускаю"), "факт обязан сообщать действие: {}", n);
    }

    /// G4: RAM-прогноз — тот же запрет факта, что и VRAM-прогноз.
    #[test]
    fn ram_forecast_never_claims_shortage_as_fact() {
        let n = ram_forecast_notice("pinned-буферы: ~5.6 ГБ, свободно 3.8 ГБ");
        assert!(!n.contains("Не хватает"), "прогноз не имеет права говорить «Не хватает»: {}", n);
        assert!(!n.contains("не хватило"), "прогноз не имеет права говорить «не хватило»: {}", n);
        assert!(n.contains("По оценке"), "прогноз обязан помечать себя оценкой: {}", n);
        assert!(n.contains("ОЗУ"), "прогноз обязан называть ресурс: {}", n);
    }
}