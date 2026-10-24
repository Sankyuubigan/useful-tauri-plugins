// Логика встроенного дашборда шлюза: вкладки, модели, релей, журнал,
// каталог пресетов и общий init.
//
// Домен провайдеров (карточки, ключи, OAuth, модалки) живёт в providers.js,
// общие хелперы — в utils.js: файлы разделены по домену, а не по размеру.
// Один источник правды — REST API шлюза (см. src/admin.rs). Страница ничего
// не запоминает между перезагрузками: после перезапуска порта/состава
// провайдеров она показывает ровно то, что реально настроено.
//
// Встраивание в приложение-хост: после любого изменения конфигурации шлюз
// сообщает родительскому окну через postMessage, и панель хоста обновляет
// свой список комбо без ожидания собственного таймера.

import {
  $, $$, state, api, toast, fail, busy, esc, plural, notifyHost,
} from './utils.js';
// Циклический импорт безопасен: это function-декларации в providers.js,
// к моменту любого вызова модуль с ними уже вычислен.
import { renderProviders, bindProviderExtras } from './providers.js';

// ────────────────────────────── загрузка ──────────────────────────────

export async function refreshAll() {
  try {
    const [status, config] = await Promise.all([api('/api/status'), api('/api/config')]);
    state.status = status;
    state.config = config;
    renderHeader();
    renderProviders();
    renderModels();
    renderRelay();
    void loadRelaySlots();
  } catch (e) {
    fail('Не удалось загрузить настройки', e);
  }
}

export async function refreshPresets() {
  state.presets = await api('/api/presets');
  renderPresetsHint();
  fillPresetSelect();
  return state.presets;
}

function renderHeader() {
  const s = state.status;
  if (!s) return;
  const c = state.config;
  const comboWord = plural(s.combos_total, 'модель', 'модели', 'моделей');
  $('#subtitle').textContent =
    `версия ${s.version} · порт ${s.port} · ${s.providers_enabled} из ${s.providers_total} провайдеров · ` +
    `${s.combos_total} ${comboWord} · работает ${Math.floor(s.uptime_sec / 60)} мин`;
  $('#relay-url').textContent = c.vercel_relay_url || 'не задан';
  $('#relay-token').textContent = c.vercel_relay_token_set
    ? 'задан (воркер требует его при каждом запросе)'
    : 'не задан — релей работать не будет';
}

// Карточка проблем конфигурации рисуется ВМЕСТЕ со списком провайдеров —
// в `renderProviders` (providers.js).
//
// Отдельный `renderProblems` тут был не нужен: он искал карточку по классу
// gw-problems, которого в разметке не бывает (карточки проблем рисует
// `renderProviders` отдельным блоком с `class="gw-problems"`), —
// поэтому селектор всегда возвращал null, функция на каждом обновлении
// вставляла ВТОРУЮ копию карточки, а первая оставалась от `renderProviders`.
// Два рендерера одного контейнера — и есть корень дублей, поэтому удалён
// именно он, а не «починен» селектор.

function renderPresetsHint() {
  const p = state.presets;
  if (!p) return;
  $('#presets-hint').textContent =
    `каталог: ${p.source} · ${p.providers.length} провайдеров · обновлён ${p.updated_at || '—'}`;
  $('#modal-presets-hint').textContent = `${p.providers.length} провайдеров (${p.source})`;
}

// ─────────────────────────────── модели ───────────────────────────────

function renderModels() {
  const list = $('#models-list');
  const ready = state.config.providers.filter(p => p.models_count > 0);
  if (!ready.length) {
    list.innerHTML = `<div class="gw-empty">Моделей пока нет.<br>` +
      `Добавьте провайдера и нажмите «Обновить модели» — список опрашивается у провайдера, а не берётся из каталога.</div>`;
    return;
  }
  list.innerHTML = ready.map(p =>
    `<div class="gw-card">` +
    `<div class="gw-card-head"><span class="gw-name">${esc(p.name)}</span>` +
    `<span class="gw-id">${esc(p.id)}</span>` +
    `<span class="gw-badge">${p.models_count} ${plural(p.models_count, 'модель', 'модели', 'моделей')}</span></div>` +
    `<div class="gw-kv"><span>В чате как</span><b class="gw-mono">${esc(p.id)}/&lt;модель&gt;</b></div>` +
    `<div class="gw-models-list">${p.models.map(m => `<span>${esc(m)}</span>`).join('')}</div>` +
    `</div>`).join('');
}

// ──────────────────────────────── релей ────────────────────────────────

function renderRelay() {
  const c = state.config;
  $('#relay-url-input').value = c.vercel_relay_url || '';
}

async function testRelay() {
  const btn = $('#btn-relay-test');
  const out = $('#relay-test-result');
  busy(btn, true);
  out.textContent = 'проверяем…';
  try {
    const r = await api('/api/relay/test', { method: 'POST' });
    out.textContent = r.reachable
      ? `релей отвечает (${r.status}, ${r.latency_ms} мс) — это наш воркер`
      : `релей ответил HTTP ${r.status}, но это не наш воркер: ${r.detail}`;
    out.style.color = r.reachable ? 'var(--success)' : 'var(--warning)';
  } catch (e) {
    out.textContent = `проверка не удалась: ${e.message}`;
    out.style.color = 'var(--danger)';
  } finally { busy(btn, false); }
}

async function deployRelay() {
  const btn = $('#btn-relay-deploy');
  const out = $('#relay-deploy-result');
  const kind = $('#relay-kind').value;
  const token = $('#relay-token-input').value.trim();
  if (!token) {
    out.textContent = kind === 'cloudflare' ? 'Введите Cloudflare API-токен' : 'Введите токен Vercel';
    out.style.color = 'var(--warning)';
    return;
  }
  busy(btn, true);
  out.textContent = 'разворачиваем…';
  try {
    const body = kind === 'cloudflare'
      ? {
          vercel_token: token,
          kind,
          script: $('#relay-script-input').value.trim() || null,
          account_id: $('#relay-account-input').value.trim() || null,
        }
      : {
          vercel_token: token,
          kind,
          project: $('#relay-project-input').value.trim() || null,
        };
    const r = await api('/api/relay/deploy', { method: 'POST', body: JSON.stringify(body) });
    out.textContent = `готово: ${r.url}`;
    out.style.color = 'var(--success)';
    $('#relay-token-input').value = '';
    $('#relay-account-input').value = '';
    await refreshAll();
    notifyHost('relay-deployed');
  } catch (e) {
    // Ошибку показываем целиком: в ней текст ответа платформы (нет прав,
    // неверный токен, «несколько аккаунтов»…), по которому видно, что не так.
    out.textContent = `не удалось: ${e.message}`;
    out.style.color = 'var(--danger)';
  } finally { busy(btn, false); }
}

/** Поля формы видны по выбранной платформе: пользователь деплоит на
 *  Cloudflare — ему не говорят «имя проекта», а просят платформенный
 *  Account ID/имя воркера. */
function relayKindSync() {
  const cf = $('#relay-kind')?.value === 'cloudflare';
  const vercelOnly = $('#relay-vercel-only');
  const cfOnly = $('#relay-cf-only');
  if (!vercelOnly || !cfOnly) return; // разметка вкладки ещё не загружена
  vercelOnly.hidden = cf;
  cfOnly.hidden = !cf;
  const cfAccount = $('#relay-cf-account');
  if (cfAccount) cfAccount.hidden = !cf;
  const tokenInput = $('#relay-token-input');
  if (tokenInput) tokenInput.placeholder = cf ? 'Cloudflare API-токен…' : 'vercel_…';
  const deployBtn = $('#btn-relay-deploy');
  if (deployBtn) deployBtn.textContent = cf ? 'Развернуть на Cloudflare' : 'Развернуть на Vercel';
}

// ─────────────────────── слоты релеев ───────────────────────

const SLOT_KIND_NAMES = { vercel: 'Vercel', cloudflare: 'Cloudflare Workers' };

async function loadRelaySlots() {
  try {
    const r = await api('/api/relay/slots');
    renderRelaySlots(r);
  } catch (e) { /* слоты не блокируют остальную страницу */ }
}

function renderRelaySlots(r) {
  const box = $('#relay-slots');
  const slots = (r && r.slots) || [];
  if (!slots.length) {
    box.innerHTML =
      '<div class="gw-empty">Релеев ещё нет — разверните авто-деплоем или вставьте URL вручную.</div>';
    return;
  }
  box.innerHTML = slots.map((s) => {
    const urlRow = s.url
      ? `<div class="gw-kv"><span>URL</span><b class="gw-break"><a href="${esc(s.url)}" target="_blank" rel="noopener">${esc(s.url)}</a></b></div>`
      : '';
    const cfRows = s.kind === 'cloudflare'
      ? (s.cf_script_name ? `<div class="gw-kv"><span>Воркер</span><b>${esc(s.cf_script_name)}</b></div>` : '') +
        (s.cf_account_id ? `<div class="gw-kv"><span>Аккаунт</span><b>${esc(s.cf_account_id)}</b></div>` : '')
      : '';
    const secret = s.relay_secret_set ? 'задан' : 'не задан — релей отвечает 503 на каждый запрос';
    return `<div class="gw-card">` +
      `<div class="gw-card-head"><span class="gw-name">${esc(SLOT_KIND_NAMES[s.kind] || s.kind)}</span>` +
      `<span class="gw-id">${esc(s.id)}</span>` +
      `${s.active ? '<span class="gw-badge">активен</span>' : ''}</div>` +
      urlRow + cfRows +
      `<div class="gw-kv"><span>Секрет RELAY_TOKEN</span><b>${secret}</b></div>` +
      `<div class="gw-row">` +
      `${s.active ? '' : `<button class="gw-ghost" data-slot-activate="${esc(s.id)}">Сделать активным</button>`}` +
      `<button class="gw-ghost" data-slot-delete="${esc(s.id)}">Удалить</button>` +
      `</div></div>`;
  }).join('');
  $$('#relay-slots [data-slot-activate]').forEach(btn =>
    btn.addEventListener('click', () => void activateRelaySlot(btn.dataset.slotActivate, btn)));
  $$('#relay-slots [data-slot-delete]').forEach(btn =>
    btn.addEventListener('click', () => void deleteRelaySlot(btn.dataset.slotDelete, btn)));
}

async function activateRelaySlot(id, btn) {
  busy(btn, true);
  try {
    await api('/api/relay/slots/activate', { method: 'POST', body: JSON.stringify({ id }) });
    toast(`Активный релей: ${id}`);
    await refreshAll();
    notifyHost('relay-slot-activated');
  } catch (e) { fail('Не удалось переключить релей', e); }
  finally { busy(btn, false); }
}

async function deleteRelaySlot(id, btn) {
  if (!window.confirm(`Удалить релей «${id}»? Если он активен, провайдеры с галочкой «через релей» перестанут работать.`)) return;
  busy(btn, true);
  try {
    await api('/api/relay/slots/delete', { method: 'POST', body: JSON.stringify({ id }) });
    toast('Релей удалён');
    await refreshAll();
    notifyHost('relay-slot-deleted');
  } catch (e) { fail('Не удалось удалить релей', e); }
  finally { busy(btn, false); }
}

async function saveRelayUrl() {
  const btn = $('#btn-relay-save');
  busy(btn, true);
  try {
    await api('/api/config', {
      method: 'PUT',
      body: JSON.stringify({ vercel_relay_url: $('#relay-url-input').value.trim() }),
    });
    toast('URL релея сохранён');
    await refreshAll();
    notifyHost('relay-url-saved');
  } catch (e) { fail('Не удалось сохранить', e); }
  finally { busy(btn, false); }
}

/**
 * Показать секрет релея для ручного развёртывания.
 *
 * Секрет лежит в `/api/relay/token`, а не в `/api/config`: конфиг отдаётся на
 * каждом обновлении страницы и виден любому локальному процессу, поэтому
 * секрет из него исключён (правило раздела «Секреты» в `admin.rs`). Здесь
 * пользователь спросил конкретно — он вписывает `RELAY_TOKEN` в панель Vercel.
 */
async function showRelayToken() {
  const btn = $('#btn-relay-show-token');
  const out = $('#relay-token-manual');
  busy(btn, true);
  try {
    const r = await api('/api/relay/token');
    out.textContent = r.token;
    out.hidden = false;
    btn.textContent = 'Скрыть секрет';
  } catch (e) {
    out.textContent = `Не удалось получить секрет: ${e.message}`;
    out.style.color = 'var(--danger)';
    out.hidden = false;
  } finally { busy(btn, false); }
}

// ──────────────────────────── каталог пресетов ────────────────────────────

/**
 * Поиск по каталогу: каталог после sync — сотни провайдеров, листать
 * `<select>` вручную непригодно. Фильтр ищет подстроку без учёта регистра по
 * имени, id и base_url.
 *
 * Выбранное значение принудительно остаётся в списке, даже если под фильтр
 * не попадает: иначе `<select>` молча терял бы выбор пользователя при каждом
 * вводе символа.
 */
export function presetMatches(p, q) {
  if (!q) return true;
  const hay = `${p.name || ''} ${p.id || ''} ${p.default_base_url || ''}`.toLowerCase();
  return hay.includes(q);
}

export function fillPresetSelect() {
  const sel = $('#preset-select');
  const search = $('#preset-search');
  const q = (search && search.value || '').trim().toLowerCase();
  const providers = (state.presets && state.presets.providers) || [];
  const current = sel.value;
  const visible = providers.filter(p => p.id === current || presetMatches(p, q));
  sel.innerHTML = '<option value="">— выберите из списка —</option>' +
    visible.map(p =>
      `<option value="${esc(p.id)}" data-base="${esc(p.default_base_url)}" data-name="${esc(p.name)}"` +
      ` data-docs="${esc(p.api_docs_url || '')}">${esc(p.name)}</option>`).join('');
  sel.value = current;
  const hint = $('#preset-count');
  if (hint) {
    hint.textContent = q
      ? `показано ${visible.length} из ${providers.length}`
      : `${providers.length} провайдеров`;
  }
}

/**
 * Обновить каталог провайдеров.
 *
 * Показывает прогресс с секундами прямо в строке каталога (шаг 3 плана):
 * источники каталога — это сеть, sync легко идёт дольше секунды, и без
 * обратной связи кнопка выглядела зависшей. При неудаче текст каталога
 * восстанавливается прежним — прежние данные не исчезают.
 */
export async function syncPresets(btn) {
  busy(btn, true);
  // Бэкап текстов до старта: именно их восстановим при ошибке.
  const hintMain = $('#presets-hint'), hintModal = $('#modal-presets-hint');
  const prevMain = hintMain.textContent, prevModal = hintModal.textContent;
  const started = Date.now();
  hintMain.textContent = 'обновляем каталог провайдеров… 0 с';
  hintModal.textContent = 'обновляем каталог… 0 с';
  const tick = setInterval(() => {
    const sec = Math.floor((Date.now() - started) / 1000);
    hintMain.textContent = `обновляем каталог провайдеров… ${sec} с`;
    hintModal.textContent = `обновляем каталог… ${sec} с`;
  }, 1000);
  try {
    const r = await api('/api/presets/sync', { method: 'POST' });
    const failed = (r.attempts || []).filter(a => !a.ok);
    let msg = `Каталог обновлён: ${r.count} провайдеров (+${r.added} / -${r.removed}), источник ${r.from_url}`;
    if (failed.length) {
      msg += `; не ответили: ${failed.map(a => a.url).join(', ')}`;
    }
    toast(msg, failed.length ? 'err' : 'ok');
    await refreshPresets();
  } catch (e) {
    fail('Не удалось обновить каталог', e);
    hintMain.textContent = prevMain;
    hintModal.textContent = prevModal;
  } finally {
    clearInterval(tick);
    busy(btn, false);
  }
}

// ──────────────────────────────── журнал ────────────────────────────────

export async function loadEvents(append = false) {
  try {
    const q = append && state.lastSeq ? `?since=${state.lastSeq}` : '?limit=200';
    const r = await api('/api/events' + q);
    if (append) state.events = state.events.concat(r.events);
    else state.events = r.events;
    state.lastSeq = r.last_seq;
    renderLog();
  } catch (e) { fail('Не удалось загрузить журнал', e); }
}

function renderLog() {
  const box = $('#log-list');
  box.innerHTML = state.events.map(e => {
    const t = new Date(e.ts_ms);
    const hh = String(t.getHours()).padStart(2, '0');
    const mm = String(t.getMinutes()).padStart(2, '0');
    const ss = String(t.getSeconds()).padStart(2, '0');
    // Level префиксуется той же конвенцией gw-: чужой CSS не должен ни на
    // один класс нашей страницы нацеливаться — в том числе на «warn»/«error».
    const lvl = e.level === 'warn' || e.level === 'error' ? `gw-${esc(e.level)}` : '';
    return `<div class="gw-line ${lvl}"><span class="gw-time">${hh}:${mm}:${ss}</span>${esc(e.message)}</div>`;
  }).join('') || '<div class="gw-hint">Журнал пуст.</div>';
  if ($('#log-autoscroll').checked) box.scrollTop = box.scrollHeight;
}

function startLogPolling() {
  if (state.logTimer) clearInterval(state.logTimer);
  state.logTimer = setInterval(() => {
    if (!$('#page-log').hidden) void loadEvents(true);
  }, 3000);
}

// ──────────────────────────────── вкладки ────────────────────────────────

function initTabs() {
  $$('#tabs button').forEach(btn => btn.addEventListener('click', () => {
    $$('#tabs button').forEach(b => b.classList.toggle('gw-active', b === btn));
    $$('.gw-page').forEach(p => { p.hidden = p.id !== `page-${btn.dataset.tab}`; });
    if (btn.dataset.tab === 'log') void loadEvents(false);
  }));
}

function init() {
  initTabs();
  bindProviderExtras();
  $('#btn-sync-presets').addEventListener('click', (e) => void syncPresets(e.currentTarget));
  $('#btn-modal-sync').addEventListener('click', (e) => void syncPresets(e.currentTarget));

  $('#btn-relay-test').addEventListener('click', () => void testRelay());
  $('#btn-relay-deploy').addEventListener('click', () => void deployRelay());
  $('#btn-relay-save').addEventListener('click', () => void saveRelayUrl());
  $('#relay-kind').addEventListener('change', relayKindSync);
  relayKindSync();
  $('#btn-relay-show-token').addEventListener('click', (e) => {
    const out = $('#relay-token-manual');
    if (out.hidden) { void showRelayToken(); return; }
    out.hidden = true;
    out.textContent = '';
    e.currentTarget.textContent = 'Показать секрет RELAY_TOKEN';
  });
  $('#btn-log-refresh').addEventListener('click', () => void loadEvents(false));

  $('#btn-restart').addEventListener('click', async () => {
    const btn = $('#btn-restart');
    if (!window.confirm('Перезапустить шлюз? Прокси в этот момент будет недоступна.')) return;
    busy(btn, true);
    try {
      await api('/api/restart', { method: 'POST' });
      toast('Шлюз перезапускается…');
      setTimeout(() => window.location.reload(), 4000);
    } catch (e) { fail('Не удалось перезапустить', e); busy(btn, false); }
  });

  void refreshAll();
  void refreshPresets().catch(() => { /* каталог не критичен для старта */ });
  startLogPolling();
}

document.addEventListener('DOMContentLoaded', init);
