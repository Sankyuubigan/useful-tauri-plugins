//! Ротация API-ключей и их кулдауны.
//!
//! ## Что здесь и почему
//!
//! Состояние ротации и кулдаунов — **рантайм**, а не конфигурация: см. комментарий
//! в [`crate::config`]. Оно не сериализуется и не выживает перезапуск шлюза.
//! Для пользователя это правильное поведение: перезапустил шлюз — получил свежие
//! ключи, а не «до истечения кулдауна ничего не работает» из вчерашнего состояния.
//!
//! ## Политика отказов
//!
//! | Отказ | Реакция | Почему так
//! |---|---|---|
//! | `401` / `403` | ключ деактивируется **на диске**, кулдаун 1 ч | ключ не станет валидным сам; повторно долбить бессмысленно
//! | `429` | кулдаун из `Retry-After`, иначе 60 с | провайдер сам сообщает темп
//! | `5xx` | кулдаун 15 с | ошибка на стороне апстрима, ключ ни при чём
//! | транспорт | кулдаун 15 с | то же
//!
//! ## Лазейный фейловер
//!
//! [`KeyRotation::candidates`] возвращает ключи в round-robin порядке, уже
//! отфильтрованные по кулдауну. Если в кулдауне оказались **все** живые ключи,
//! [`KeyRotation::reset_if_all_sleeping`] один раз сбрасывает кулдауны — иначе
//! шлюз молча деградирует в «503 у всех, кто бы ни спросил», хотя через минуту
//! всё заработало бы само.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Тип отказа апстрима и выведенная из него длительность кулдауна.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// `401` / `403` — ключ невалиден. Деактивируется персистентно.
    Auth,
    /// `429` — лимит запросов.
    RateLimit,
    /// `5xx` — ошибка на стороне провайдера.
    Server,
    /// Таймаут/обрыв соединения.
    Transport,
}

/// Кулдаун для `401/403`. Ключ дополнительно выключается на диске, поэтому
/// значение только перестраховка на тот случай, если пользователь снова включит
/// его вручную до истечения часа.
pub const AUTH_COOLDOWN: Duration = Duration::from_secs(3600);
/// Кулдаун для `5xx` и транспортных ошибок.
pub const TRANSIENT_COOLDOWN: Duration = Duration::from_secs(15);
/// Кулдаун для `429` без заголовка `Retry-After`.
pub const DEFAULT_RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);

impl FailureKind {
    /// Классифицировать HTTP-статус ответа апстрима.
    pub fn from_status(status: u16) -> Option<FailureKind> {
        match status {
            401 | 403 => Some(FailureKind::Auth),
            429 => Some(FailureKind::RateLimit),
            500..=599 => Some(FailureKind::Server),
            _ => None,
        }
    }

    /// Длительность кулдауна. `retry_after` (из заголовка `Retry-After`) важнее
    /// значения по умолчанию для `RateLimit`; для остальных типов игнорируется —
    /// провайдер, вернувший 503 с `Retry-After: 600`, не запрещает пробовать
    /// раньше, но и не гарантирует, что через 15 с будет лучше.
    pub fn cooldown(self, retry_after: Option<Duration>) -> Duration {
        match self {
            FailureKind::Auth => AUTH_COOLDOWN,
            FailureKind::RateLimit => retry_after.unwrap_or(DEFAULT_RATE_LIMIT_COOLDOWN),
            FailureKind::Server | FailureKind::Transport => TRANSIENT_COOLDOWN,
        }
    }

    /// Нужно ли персистентно выключить ключ.
    pub fn deactivates_key(self) -> bool {
        matches!(self, FailureKind::Auth)
    }
}

/// Разобрать заголовок `Retry-After`. Поддерживаются обе формы RFC 9110:
/// целые секунды и HTTP-дата. Неразобранное значение игнорируется (`None`).
pub fn parse_retry_after(raw: Option<&str>) -> Option<Duration> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(secs) = raw.parse::<u64>() {
        // Бессмысленно ждать сутки из-за одного 429 — ограничиваем сверху.
        return Some(Duration::from_secs(secs.min(3600)));
    }
    // HTTP-дата: "Wed, 21 Oct 2015 07:28:00 GMT". Разбираем вручную, чтобы не
    // тащить http::date ради одной строки.
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let day: u32 = parts[1].parse().ok()?;
    let month = crate_month_index(parts[2])?;
    let year: i32 = parts[3].parse().ok()?;
    let hms: Vec<&str> = parts[4].split(':').collect();
    if hms.len() != 3 {
        return None;
    }
    let hour: u64 = hms[0].parse().ok()?;
    let minute: u64 = hms[1].parse().ok()?;
    let second: u64 = hms[2].parse().ok()?;
    let target = utc_seconds(year, month, day, hour, minute, second)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if target <= now {
        return Some(Duration::from_secs(0));
    }
    Some(Duration::from_secs((target - now).min(3600) as u64))
}

fn crate_month_index(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS.iter().position(|m| m.eq_ignore_ascii_case(name)).map(|i| i as u32 + 1)
}

/// Секунды с Unix-эпохи для UTC-даты. Алгоритм дней от Гонконга (Howard Hinnant).
fn utc_seconds(year: i32, month: u32, day: u32, hour: u64, minute: u64, second: u64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((month + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    Some(secs)
}

/// Ключ кулдауна: пара «провайдер + ключ».
///
/// Пара, а не только id ключа: id ключей у разных провайдеров не обязаны быть
/// уникальны глобально, и коллизия молча отправила бы чужой ключ в кулдаун.
type CooldownKey = (String, String);

/// Рантайм-состояние ротации ключей.
///
/// Ключи — `(provider_id, key_id)`, значения — момент, до которого ключ
/// «спит». Потокобезопасно: `AtomicUsize` для счётчика, `Mutex` для кулдаунов.
#[derive(Debug)]
pub struct KeyRotation {
    rr: AtomicUsize,
    cooldown: Mutex<HashMap<CooldownKey, Instant>>,
}

impl Default for KeyRotation {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyRotation {
    pub fn new() -> Self {
        Self { rr: AtomicUsize::new(0), cooldown: Mutex::new(HashMap::new()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CooldownKey, Instant>> {
        // Отравленный мьютекс = паника в другом потоке. Внутри нет инвариантов,
        // которые могли бы быть нарушены, поэтому восстанавливаемся.
        self.cooldown.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Момент, до которого ключ спит.
    pub fn cooldown_until(&self, provider_id: &str, key_id: &str) -> Option<Instant> {
        self.lock().get(&(provider_id.to_string(), key_id.to_string())).copied()
    }

    /// Сколько осталось ждать (ноль, если ключ готов).
    pub fn cooldown_remaining(&self, provider_id: &str, key_id: &str) -> Option<Duration> {
        let until = self.cooldown_until(provider_id, key_id)?;
        Some(until.saturating_duration_since(Instant::now()))
    }

    /// Ключ готов к использованию прямо сейчас.
    pub fn is_available(&self, provider_id: &str, key_id: &str) -> bool {
        match self.cooldown_until(provider_id, key_id) {
            None => true,
            Some(until) => Instant::now() >= until,
        }
    }

    /// Отметить отказ: поставить ключ в кулдаун.
    pub fn mark_failure(
        &self,
        provider_id: &str,
        key_id: &str,
        kind: FailureKind,
        retry_after: Option<Duration>,
    ) {
        let until = Instant::now() + kind.cooldown(retry_after);
        let mut map = self.lock();
        map.insert((provider_id.to_string(), key_id.to_string()), until);
        // Кулдауны не растут бесконечно: у провайдера с одним ключом и частыми
        // 429 накопятся старые записи. Чистим всё, что уже истекло.
        if map.len() > 64 {
            let now = Instant::now();
            map.retain(|_, until| *until > now);
        }
    }

    /// Снять кулдаун с ключа (например, после ручного «попробовать снова»).
    pub fn clear_cooldown(&self, provider_id: &str, key_id: &str) {
        self.lock().remove(&(provider_id.to_string(), key_id.to_string()));
    }

    /// Сбросить все кулдауны (используется при ручном перезапуске шлюза из UI).
    pub fn clear_all(&self) {
        self.lock().clear();
        self.rr.store(0, Ordering::Relaxed);
    }

    /// Кандидаты для очередной попытки: id активных ключей в round-robin
    /// порядке, без ключей в кулдауне.
    ///
    /// Порядок циклически сдвигается на счётчик, поэтому при нескольких ключах
    /// нагрузка делится равномерно, а при повторных попытках внутри одного
    /// запроса следующим берётся **следующий** ключ — это и есть лазейный
    /// фейловер.
    pub fn candidates(&self, provider_id: &str, keys: &[crate::AccountKey]) -> Vec<String> {
        let all: Vec<String> = keys
            .iter()
            .filter(|k| k.is_active && !k.key.trim().is_empty())
            .map(|k| k.id.clone())
            .collect();
        if all.is_empty() {
            return Vec::new();
        }
        let offset = self.rr.fetch_add(1, Ordering::Relaxed) % all.len();
        let mut rotated: Vec<String> = Vec::with_capacity(all.len());
        rotated.extend(all[offset..].iter().cloned());
        rotated.extend(all[..offset].iter().cloned());
        rotated
            .into_iter()
            .filter(|id| self.is_available(provider_id, id))
            .collect()
    }

    /// Все живые ключи спят? Тогда один раз сбрасываем их кулдауны.
    ///
    /// Возвращает `true`, если кулдауны были сброшены. Деактивированные на диске
    /// ключи (`is_active == false`) не «разбуживаются» — они выключены
    /// осознанно, иначе шлюз вернулся бы к бесконечному циклу `401`.
    pub fn reset_if_all_sleeping(
        &self,
        provider_id: &str,
        keys: &[crate::AccountKey],
    ) -> bool {
        let alive: Vec<&str> = keys
            .iter()
            .filter(|k| k.is_active && !k.key.trim().is_empty())
            .map(|k| k.id.as_str())
            .collect();
        if alive.is_empty() {
            return false;
        }
        if alive.iter().any(|id| self.is_available(provider_id, id)) {
            return false;
        }
        let mut map = self.lock();
        for id in &alive {
            map.remove(&(provider_id.to_string(), (*id).to_string()));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountKey;

    fn key(id: &str, active: bool) -> AccountKey {
        AccountKey { id: id.to_string(), key: format!("sk-{id}"), is_active: active }
    }

    #[test]
    fn failure_kind_from_status_classifies_correctly() {
        assert_eq!(FailureKind::from_status(401), Some(FailureKind::Auth));
        assert_eq!(FailureKind::from_status(403), Some(FailureKind::Auth));
        assert_eq!(FailureKind::from_status(429), Some(FailureKind::RateLimit));
        assert_eq!(FailureKind::from_status(500), Some(FailureKind::Server));
        assert_eq!(FailureKind::from_status(503), Some(FailureKind::Server));
        // 4xx, кроме 401/403/429 — это ошибка ЗАПРОСА, ключ тут ни при чём.
        assert_eq!(FailureKind::from_status(400), None);
        assert_eq!(FailureKind::from_status(404), None);
        assert_eq!(FailureKind::from_status(200), None);
    }

    #[test]
    fn only_auth_deactivates_key() {
        assert!(FailureKind::Auth.deactivates_key());
        assert!(!FailureKind::RateLimit.deactivates_key());
        assert!(!FailureKind::Server.deactivates_key());
        assert!(!FailureKind::Transport.deactivates_key());
    }

    #[test]
    fn retry_after_overrides_rate_limit_default() {
        assert_eq!(FailureKind::RateLimit.cooldown(Some(Duration::from_secs(5))), Duration::from_secs(5));
        assert_eq!(FailureKind::RateLimit.cooldown(None), DEFAULT_RATE_LIMIT_COOLDOWN);
        // Retry-After игнорируется для не-429: провайдер не обязан его выставлять.
        assert_eq!(FailureKind::Server.cooldown(Some(Duration::from_secs(5))), TRANSIENT_COOLDOWN);
    }

    #[test]
    fn parse_retry_after_seconds_form() {
        assert_eq!(parse_retry_after(Some("30")), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after(Some("  7 ")), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(Some("")), None);
        assert_eq!(parse_retry_after(None), None);
        assert_eq!(parse_retry_after(Some("позже")), None);
    }

    #[test]
    fn parse_retry_after_clamps_absurd_values() {
        assert_eq!(parse_retry_after(Some("999999")), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn parse_retry_after_http_date_form() {
        // Дата в прошлом → ждать нечего.
        assert_eq!(parse_retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), Some(Duration::from_secs(0)));
        // Дата в будущем → разумное положительное значение, не 0 и не 3600.
        let future = parse_retry_after(Some("Wed, 21 Oct 2099 07:28:00 GMT")).expect("date");
        assert!(future > Duration::from_secs(60), "{:?}", future);
        assert!(future <= Duration::from_secs(3600), "{:?}", future);
    }

    #[test]
    fn round_robin_cycles_through_all_keys() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", true), key("b", true), key("c", true)];
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(rot.candidates("groq", &keys));
        }
        // Каждый проход отдаёт все три ключа, и стартовая точка сдвигается.
        for s in &seen {
            assert_eq!(s.len(), 3, "{:?}", s);
        }
        assert_ne!(seen[0][0], seen[1][0], "счётчик ротации не сдвинулся: {:?}", seen);
        let mut unique_starts = std::collections::BTreeSet::new();
        for s in &seen {
            unique_starts.insert(s[0].clone());
        }
        assert_eq!(unique_starts.len(), 3, "не все ключи побывали первыми: {:?}", seen);
    }

    #[test]
    fn cooldown_key_is_skipped_by_candidates() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", true), key("b", true)];
        rot.mark_failure("groq", "a", FailureKind::RateLimit, Some(Duration::from_secs(60)));
        for _ in 0..4 {
            assert_eq!(rot.candidates("groq", &keys), vec!["b".to_string()]);
        }
        assert!(!rot.is_available("groq", "a"));
        assert!(rot.is_available("groq", "b"));
    }

    #[test]
    fn cooldown_is_scoped_per_provider() {
        let rot = KeyRotation::new();
        rot.mark_failure("groq", "a", FailureKind::Auth, None);
        let keys = vec![key("a", true)];
        // Тот же id ключа у другого провайдера обязан остаться доступным.
        assert_eq!(rot.candidates("deepseek", &keys).len(), 1);
        assert!(rot.candidates("groq", &keys).is_empty());
    }

    #[test]
    fn deactivated_key_is_never_a_candidate() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", false), key("b", true)];
        assert_eq!(rot.candidates("groq", &keys), vec!["b".to_string()]);
    }

    #[test]
    fn reset_only_when_every_live_key_sleeps() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", true), key("b", true)];

        // Ничего не спит → сбрасывать нечего.
        assert!(!rot.reset_if_all_sleeping("groq", &keys));

        rot.mark_failure("groq", "a", FailureKind::RateLimit, Some(Duration::from_secs(60)));
        assert!(!rot.reset_if_all_sleeping("groq", &keys), "b ещё не спит");

        rot.mark_failure("groq", "b", FailureKind::RateLimit, Some(Duration::from_secs(60)));
        assert!(rot.reset_if_all_sleeping("groq", &keys));
        assert_eq!(rot.candidates("groq", &keys).len(), 2);
        // Второй раз — уже нечего сбрасывать.
        assert!(!rot.reset_if_all_sleeping("groq", &keys));
    }

    #[test]
    fn reset_does_not_revive_deactivated_keys() {
        let rot = KeyRotation::new();
        let keys = vec![key("dead", false), key("b", true)];
        rot.mark_failure("groq", "dead", FailureKind::Auth, None);
        rot.mark_failure("groq", "b", FailureKind::Auth, None);
        // «b» спит, «dead» выключен на диске. Сброс поднимает только «b».
        assert!(rot.reset_if_all_sleeping("groq", &keys));
        assert_eq!(rot.candidates("groq", &keys), vec!["b".to_string()]);
    }

    #[test]
    fn reset_is_noop_when_all_keys_deactivated() {
        let rot = KeyRotation::new();
        let keys = vec![key("dead1", false), key("dead2", false)];
        assert!(!rot.reset_if_all_sleeping("groq", &keys));
    }

    #[test]
    fn clear_cooldown_makes_key_available_again() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", true)];
        rot.mark_failure("groq", "a", FailureKind::Auth, None);
        assert!(rot.candidates("groq", &keys).is_empty());
        rot.clear_cooldown("groq", "a");
        assert_eq!(rot.candidates("groq", &keys).len(), 1);
    }

    #[test]
    fn expired_cooldown_becomes_available() {
        let rot = KeyRotation::new();
        let keys = vec![key("a", true)];
        rot.mark_failure("groq", "a", FailureKind::RateLimit, Some(Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(rot.candidates("groq", &keys).len(), 1);
    }

    #[test]
    fn cooldown_map_does_not_grow_without_bound() {
        let rot = KeyRotation::new();
        // 200 разных ключей с нулевым кулдауном: истёкшие записи должны вычищаться.
        for i in 0..200 {
            rot.mark_failure("groq", &format!("k{i}"), FailureKind::RateLimit, Some(Duration::from_millis(0)));
        }
        assert!(rot.lock().len() < 200, "истёкшие кулдауны не вычищены: {}", rot.lock().len());
    }
}