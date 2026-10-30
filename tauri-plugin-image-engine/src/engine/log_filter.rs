//! Глушитель шума stdout sd-server: прогресс-бары и построчные отчёты
//! загрузчика тензоров дают сотни почти одинаковых строк на каждую генерацию
//! и засоряли лог хоста (king_orch.log). Ошибки проходят всегда; повторы
//! подряд одинаковых строк сворачиваются в один маркер ×N.

/// true — строка шумовая и глушится (строки с ошибками шумом не считаются).
pub fn is_noise_line(text: &str) -> bool {
    if text.contains("[ERROR]") || text.contains("error:") {
        return false;
    }
    // Прогресс-бары sd.cpp: ANSI clear-line (ESC[K) и ASCII-бар «|###».
    if text.contains('\u{1b}') || text.contains("|##") {
        return true;
    }
    // Построчный отчёт загрузчика тензоров (десятки за один спавн; цифры
    // чуть меняются, поэтому точная свёртка их не ловит).
    if text.starts_with("loading tensors completed") {
        return true;
    }
    false
}

/// Свёртка подряд идущих одинаковых строк: первая печатается, повторные —
/// одним маркером при смене строки.
pub struct LineDeduper {
    last: Option<String>,
    repeats: usize,
}

impl LineDeduper {
    pub fn new() -> Self {
        Self {
            last: None,
            repeats: 0,
        }
    }

    /// 0..2 строки для печати: маркер повторов (если были) и/или новая строка.
    pub fn feed(&mut self, text: &str) -> Vec<String> {
        if self.last.as_deref() == Some(text) {
            self.repeats += 1;
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.repeats > 0 {
            out.push(format!("⟲ предыдущая строка повторилась ×{}", self.repeats));
            self.repeats = 0;
        }
        out.push(text.to_string());
        self.last = Some(text.to_string());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_bars_and_tensor_loader_are_noise() {
        assert!(is_noise_line(
            "|####     |  10/10 - 973.84MB/s\u{1b}[K"
        ));
        assert!(is_noise_line("|###       |  33/551 - 929.82MB/s[K"));
        assert!(is_noise_line(
            "loading tensors completed, taking 0.21s (read: 0.04s, memcpy: 0.00s, convert: 0.00s, copy_to_backend: 0.01s) --- model_loader.cpp:1427"
        ));
        assert!(!is_noise_line(
            "sampling completed, taking 104.29s --- video.cpp:1776"
        ));
        // Ошибка проходит даже внутри шумовой формы.
        assert!(!is_noise_line("loading tensors completed [ERROR] oom"));
    }

    #[test]
    fn deduper_collapses_consecutive_repeats() {
        let mut d = LineDeduper::new();
        assert_eq!(d.feed("a").len(), 1);
        assert!(d.feed("a").is_empty());
        assert!(d.feed("a").is_empty());
        let out = d.feed("b");
        assert_eq!(out.len(), 2, "маркер повторов + новая строка");
        assert!(out[0].contains("×2"), "{}", out[0]);
        assert_eq!(out[1], "b");
        // После смены строки счётчик обнулён: одиночная строка не дублирует маркер.
        assert_eq!(d.feed("c").len(), 1);
    }
}
