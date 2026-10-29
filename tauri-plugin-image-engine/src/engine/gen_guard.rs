//! Гвардия единовременной media-генерации (image + video).
//!
//! sd-server держит пики RAM/VRAM всего стека весов: два одновременных
//! прогона (две вкладки, агент + UI-панель) удваивают пики и гарантированно
//! ломают машину с 16–32 ГБ ОЗУ. Атомарный флаг делает вторую генерацию
//! структурно невозможной: acquire → честный отказ с объяснением, release —
//! в Drop гвардии (успех, ошибка, отмена, паника — любой путь выхода).

use std::sync::atomic::{AtomicBool, Ordering};

static MEDIA_GEN_BUSY: AtomicBool = AtomicBool::new(false);

/// Захват слота генерации. Второй захват до освобождения — честный отказ.
#[derive(Debug)]
pub struct MediaGenGuard;

impl MediaGenGuard {
    pub fn acquire() -> Result<Self, String> {
        MEDIA_GEN_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| MediaGenGuard)
            .map_err(|_| {
                "Движок генерации занят другой задачей (изображение/видео). \
                 Дождитесь завершения текущей генерации."
                    .to_string()
            })
    }

    /// Занят ли слот прямо сейчас (для статуса в UI/логах, без захвата).
    pub fn is_busy() -> bool {
        MEDIA_GEN_BUSY.load(Ordering::SeqCst)
    }
}

impl Drop for MediaGenGuard {
    fn drop(&mut self) {
        MEDIA_GEN_BUSY.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Один последовательный тест: guard — глобальный флаг процесса, cargo
    /// гоняет тесты модуля параллельно, и два теста воевали бы за слот.
    #[test]
    fn acquire_reject_reacquire_and_panic_unwind_lifecycle() {
        // 1) обычный цикл: захват → отказ второму → освобождение → снова захват.
        let g = MediaGenGuard::acquire().expect("первый захват успешен");
        assert!(MediaGenGuard::is_busy());
        let err = MediaGenGuard::acquire().unwrap_err();
        assert!(err.contains("занят"), "отказ объясняет причину: {}", err);
        drop(g);
        assert!(!MediaGenGuard::is_busy());

        // 2) паника в scope с гвардией: unwind обязан вызвать Drop локальной
        // гвардии и освободить слот. Гвардия в payload паники НЕ дропается
        // до освобождения payload'а — это не боевой сценарий (guard живёт
        // в локальной переменной), поэтому panic_any с гвардией не тестируем.
        let result = std::panic::catch_unwind(|| {
            let _g = MediaGenGuard::acquire().expect("захват в паникующем scope");
            assert!(MediaGenGuard::is_busy());
            panic!("boom"); // _g освобождается unwind'ом
        });
        assert!(result.is_err(), "паника случилась");
        drop(result);
        assert!(!MediaGenGuard::is_busy(), "Drop гвардии освободил слот");
        let _g = MediaGenGuard::acquire().expect("слот снова доступен");
    }
}
