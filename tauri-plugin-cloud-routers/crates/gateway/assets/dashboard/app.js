// Логика встроенного дашборда шлюза.
//
// Один источник правды — REST API шлюза (см. src/admin.rs). Страница ничего не
// запоминает между перезагрузками: после перезапуска порта/состава провайдеров
// она показывает ровно то, что реально настроено.
//
// Встраивание в приложение-хост: после любого изменения конфигурации шлюз
// сообщает родительскому окну через postMessage, и панель хоста обновляет свой
// список комбо без ожидания собственного таймера.

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const state = {
  config: null,
  status: null,
  presets: null,
  events: [],
  lastSeq: 0,
  logTimer: null,
};

// ────────────────────────────── утилиты ──────────────────────────────

async function api(path, options = {}) {
  const res = await fetch(path, {
    headers: { 'content-type': 'application/json' },
    ...options,
  });
  const text = await res.text();
  let body = null;
  if (text) {
    try { body = JSON.parse(text); } catch { body = { error: text }; }
  }
  if (!res.ok) {
    throw new Error((body && body.error) || `HTTP ${res.status}`);
  }
  return body;
}

function toast(message, kind = 'ok') {
  const el = $('#toast');
  el.textContent = message;
  el.className = `toast ${kind === 'err' ? 'err' : 'ok'}`;
  el.hidden = false;
  clearTimeout(el._t);
  el._t = setTimeout(() => { el.hidden = true; }, 4200);
}

function fail(where, e) {
  console.error(where, e);
  toast(`${where}: ${e.message || e}`, 'err');
}

function busy(selector, on) {
  const el = typeof selector === 'string' ? $(selector) : selector;
  if (!el) return;
  el.disabled = on;
  el.classList.toggle('busy', on);
}

function esc(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;')
    .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function plural(n, one, few, many) {
  const m10 = n % 10, m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 10 || m100 >= 20)) return few;
  return many;
}

/** Сообщить хосту, что список провайдеров изменился. */
function notifyHost(reason) {
  if (window.parent && window.parent !== window) {
    try {
      window.parent.postMessage(
        { source: 'cloud-routers-gateway', type: 'config-changed', reason },
        '*'
      );
    } catch { /* хост может быть в песочнице — это не повод ломать UI */ }
  }
}

// ────────────────────────────── загрузка ──────────────────────────────

async function refreshAll() {
  try {
    const [status, config] = await Promise.all([api('/api/status'), api('/api/config')]);
    state.status = status;
    state.config = config;
    renderHeader();
    renderProviders();
    renderModels();
    renderRelay();
  } catch (e) {
    fail('Не удалось загрузить настройки', e);
  }
}

async function refreshPresets() {
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
// в `renderProviders`.
//
// Отдельный `renderProblems` тут был не нужен: он искал карточку по классу
// `.problems-card`, которого в разметке не бывает (`class="card problems"`), —
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

// ───────────────────────────── провайдеры ─────────────────────────────

function providerCard(p) {
  const status = p.routable
    ? '<span class="badge ok">используется</span>'
    : (!p.is_enabled
        ? '<span class="badge">выключен</span>'
        : (!p.keys.length
            ? '<span class="badge warn">нужен ключ</span>'
            : (p.keys.some(k => k.is_active)
                ? '<span class="badge warn">нажмите «Обновить модели»</span>'
                : '<span class="badge err">все ключи выключены</span>')));
  const cooldownByKey = new Map(p.cooldowns.map(c => [c.key_id, c.remaining_sec]));

  const keys = p.keys.map((k) => {
    const cd = cooldownByKey.get(k.id);
    const badges = [
      k.is_active ? '' : '<span class="badge err">выключен</span>',
      cd ? `<span class="badge warn">пауза ${cd} с</span>` : '',
    ].join(' ');
    return (
      `<div class="key ${k.is_active ? '' : 'off'}">` +
      `<span class="val">${esc(k.masked)}</span>` +
      badges +
      `<span class="spacer"></span>` +
      `<button class="ghost btn-key-reset" data-provider="${esc(p.id)}" data-key="${esc(k.id)}"` +
      `${k.is_active && cd ? '' : ' hidden'}>Снять паузу</button>` +
      `<button class="danger btn-key-del" data-provider="${esc(p.id)}" data-key="${esc(k.id)}">Удалить</button>` +
      `</div>`
    );
  }).join('');

  const models = p.models.length
    ? `<div class="models-list">${p.models.slice(0, 40).map(m => `<span>${esc(m)}</span>`).join('')}` +
      (p.models.length > 40 ? `<span>…и ещё ${p.models.length - 40}</span>` : '') + `</div>`
    : '';

  return (
    `<div class="card ${p.is_enabled ? '' : 'off'}">` +
    `<div class="card-head">` +
    `<span class="name">${esc(p.name)}</span>` +
    `<span class="id">${esc(p.id)}</span>` +
    status +
    `<span class="spacer"></span>` +
    `<label class="inline"><input type="checkbox" class="prov-toggle" data-provider="${esc(p.id)}"` +
    `${p.is_enabled ? ' checked' : ''}> включён</label>` +
    `<label class="inline"><input type="checkbox" class="prov-relay" data-provider="${esc(p.id)}"` +
    `${p.use_relay ? ' checked' : ''}> через релей</label>` +
    `</div>` +

    `<div class="kv"><span>Адрес</span><b class="break">${esc(p.base_url)}</b></div>` +
    `<div class="kv"><span>Моделей</span><b>${p.models_count}</b></div>` +
    `<div class="kv"><span>Ключей</span><b>${p.keys.length} (активных ${p.keys.filter(k => k.is_active).length})</b></div>` +
    models +
    `<div class="row">` +
    `<button class="primary btn-models" data-provider="${esc(p.id)}">Обновить модели</button>` +
    `<button class="ghost btn-test" data-provider="${esc(p.id)}">Проверить связь</button>` +
    `<button class="ghost btn-keys-add" data-provider="${esc(p.id)}">Добавить ключ</button>` +
    `<button class="danger btn-prov-del" data-provider="${esc(p.id)}">Удалить провайдера</button>` +
    `</div>` +
    `<div class="keys">${keys || '<div class="hint">Ключей нет — добавьте хотя бы один.</div>'}</div>` +
    `</div>`
  );
}

function renderProviders() {
  const list = $('#providers-list');
  const problems = (state.config.problems || []).length
    ? `<div class="card problems"><h3>Проблемы конфигурации</h3>` +
      state.config.problems.map(p => `<div class="hint">• ${esc(p)}</div>`).join('') + `</div>`
    : '';
  const body = state.config.providers.length
    ? state.config.providers.map(providerCard).join('')
    : `<div class="empty">Провайдеров пока нет.<br>` +
      `Нажмите «Добавить провайдера», выберите сервис из каталога, вставьте API-ключ ` +
      `и нажмите «Обновить модели».</div>`;
  list.innerHTML = problems + body;
  bindProviderButtons();
}

function bindProviderButtons() {
  $$('.prov-toggle').forEach(el => el.addEventListener('change', () =>
    patchProvider(el.dataset.provider, { is_enabled: el.checked })));
  $$('.prov-relay').forEach(el => el.addEventListener('change', () =>
    patchProvider(el.dataset.provider, { use_relay: el.checked })));

  $$('.btn-models').forEach(el => el.addEventListener('click', () => refreshModels(el)));
  $$('.btn-test').forEach(el => el.addEventListener('click', () => testProvider(el)));
  $$('.btn-prov-del').forEach(el => el.addEventListener('click', () => deleteProvider(el)));
  $$('.btn-key-del').forEach(el => el.addEventListener('click', () => deleteKey(el)));
  $$('.btn-key-reset').forEach(el => el.addEventListener('click', () => resetKey(el)));

  $$('.btn-keys-add').forEach(el => el.addEventListener('click', async () => {
    const id = el.dataset.provider;
    const key = window.prompt('API-ключ провайдера:');
    if (!key || !key.trim()) return;
    busy(el, true);
    try {
      await api(`/api/providers/${encodeURIComponent(id)}/keys`, {
        method: 'POST',
        body: JSON.stringify({ keys: [key.trim()] }),
      });
      toast('Ключ добавлен');
      await refreshAll();
      notifyHost('key-added');
    } catch (e) { fail('Не удалось добавить ключ', e); }
    finally { busy(el, false); }
  }));
}

async function patchProvider(id, patch) {
  try {
    await api(`/api/providers/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    });
    await refreshAll();
    notifyHost('provider-patched');
  } catch (e) { fail('Не удалось сохранить', e); }
}

async function refreshModels(btn) {
  const id = btn.dataset.provider;
  busy(btn, true);
  try {
    const r = await api(`/api/providers/${encodeURIComponent(id)}/models/refresh`, { method: 'POST' });
    toast(`Получено ${r.count} ${plural(r.count, 'модель', 'модели', 'моделей')} за ${r.latency_ms} мс`);
    await refreshAll();
    notifyHost('models-changed');
  } catch (e) { fail('Не удалось получить модели', e); }
  finally { busy(btn, false); }
}

async function testProvider(btn) {
  const id = btn.dataset.provider;
  busy(btn, true);
  try {
    const r = await api(`/api/providers/${encodeURIComponent(id)}/test`, { method: 'POST' });
    toast(`Связь есть: ${r.latency_ms} мс, моделей ${r.models_count}${r.via_relay ? ' (через релей)' : ''}`);
  } catch (e) { fail('Проверка не удалась', e); }
  finally { busy(btn, false); }
}

async function deleteProvider(btn) {
  const id = btn.dataset.provider;
  const name = (state.config.providers.find(p => p.id === id) || {}).name || id;
  if (!window.confirm(`Удалить провайдера «${name}» вместе со всеми его ключами?`)) return;
  busy(btn, true);
  try {
    await api(`/api/providers/${encodeURIComponent(id)}`, { method: 'DELETE' });
    toast('Провайдер удалён');
    await refreshAll();
    notifyHost('provider-deleted');
  } catch (e) { fail('Не удалось удалить', e); }
  finally { busy(btn, false); }
}

async function deleteKey(btn) {
  const { provider, key } = btn.dataset;
  if (!window.confirm('Удалить этот ключ?')) return;
  busy(btn, true);
  try {
    await api(`/api/providers/${encodeURIComponent(provider)}/keys/${encodeURIComponent(key)}`,
      { method: 'DELETE' });
    toast('Ключ удалён');
    await refreshAll();
    notifyHost('key-deleted');
  } catch (e) { fail('Не удалось удалить ключ', e); }
  finally { busy(btn, false); }
}

async function resetKey(btn) {
  const { provider, key } = btn.dataset;
  busy(btn, true);
  try {
    await api(`/api/providers/${encodeURIComponent(provider)}/keys/${encodeURIComponent(key)}/reset`,
      { method: 'POST' });
    toast('Пауза снята, ключ включён');
    await refreshAll();
    notifyHost('key-reset');
  } catch (e) { fail('Не удалось снять паузу', e); }
  finally { busy(btn, false); }
}

// ─────────────────────────────── модели ───────────────────────────────

function renderModels() {
  const list = $('#models-list');
  const ready = state.config.providers.filter(p => p.models_count > 0);
  if (!ready.length) {
    list.innerHTML = `<div class="empty">Моделей пока нет.<br>` +
      `Добавьте провайдера и нажмите «Обновить модели» — список опрашивается у провайдера, а не берётся из каталога.</div>`;
    return;
  }
  list.innerHTML = ready.map(p =>
    `<div class="card">` +
    `<div class="card-head"><span class="name">${esc(p.name)}</span>` +
    `<span class="id">${esc(p.id)}</span>` +
    `<span class="badge">${p.models_count} ${plural(p.models_count, 'модель', 'модели', 'моделей')}</span></div>` +
    `<div class="kv"><span>В чате как</span><b class="mono">${esc(p.id)}/&lt;модель&gt;</b></div>` +
    `<div class="models-list">${p.models.map(m => `<span>${esc(m)}</span>`).join('')}</div>` +
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
  const token = $('#relay-token-input').value.trim();
  if (!token) {
    out.textContent = 'Введите токен Vercel';
    out.style.color = 'var(--warning)';
    return;
  }
  busy(btn, true);
  out.textContent = 'разворачиваем…';
  try {
    const r = await api('/api/relay/deploy', {
      method: 'POST',
      body: JSON.stringify({
        vercel_token: token,
        project: $('#relay-project-input').value.trim() || null,
      }),
    });
    out.textContent = `готово: ${r.url}`;
    out.style.color = 'var(--success)';
    $('#relay-token-input').value = '';
    await refreshAll();
    notifyHost('relay-deployed');
  } catch (e) {
    // Ошибку показываем целиком: в ней текст ответа Vercel API, по которому
    // видно, что именно не так (нет прав, неверный токен, лимит).
    out.textContent = `не удалось: ${e.message}`;
    out.style.color = 'var(--danger)';
  } finally { busy(btn, false); }
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

function fillPresetSelect() {
  const sel = $('#preset-select');
  const providers = (state.presets && state.presets.providers) || [];
  sel.innerHTML = '<option value="">— выберите из списка —</option>' +
    providers.map(p =>
      `<option value="${esc(p.id)}" data-base="${esc(p.default_base_url)}" data-name="${esc(p.name)}"` +
      ` data-docs="${esc(p.api_docs_url || '')}">${esc(p.name)}</option>`).join('');
}

async function syncPresets(btn) {
  busy(btn, true);
  try {
    const r = await api('/api/presets/sync', { method: 'POST' });
    toast(`Каталог обновлён: ${r.count} провайдеров (+${r.added} / -${r.removed})`);
    await refreshPresets();
  } catch (e) { fail('Не удалось обновить каталог', e); }
  finally { busy(btn, false); }
}

// ─────────────────────── модалка добавления провайдера ───────────────────────

function openAddModal() {
  $('#new-id').value = '';
  $('#new-name').value = '';
  $('#new-base-url').value = '';
  $('#new-keys').value = '';
  $('#new-docs').hidden = true;
  $('#preset-select').value = '';
  // Показываем окно ДО загрузки каталога. Раньше `await refreshPresets()` стоял
  // выше этой строки, и любая задержка или сбой сети съедал показ целиком:
  // кнопка «Добавить провайдера» выглядела как нерабочая.
  $('#add-modal').hidden = false;
  if (!state.presets) {
    refreshPresets().catch((e) => fail('Не удалось загрузить каталог провайдеров', e));
  }
}

function closeAddModal() {
  $('#add-modal').hidden = true;
}

async function saveNewProvider() {
  const btn = $('#btn-modal-save');
  const body = {
    preset_id: $('#preset-select').value || null,
    id: $('#new-id').value.trim() || null,
    name: $('#new-name').value.trim() || null,
    base_url: $('#new-base-url').value.trim() || null,
    keys: $('#new-keys').value.split('\n').map(s => s.trim()).filter(Boolean),
  };
  if (!body.preset_id && !body.base_url) {
    toast('Выберите провайдера из каталога или впишите адрес вручную', 'err');
    return;
  }
  busy(btn, true);
  try {
    await api('/api/providers', { method: 'POST', body: JSON.stringify(body) });
    closeAddModal();
    toast('Провайдер добавлен. Нажмите «Обновить модели».');
    await refreshAll();
    notifyHost('provider-added');
  } catch (e) { fail('Не удалось добавить провайдера', e); }
  finally { busy(btn, false); }
}

// ──────────────────────────────── журнал ────────────────────────────────

async function loadEvents(append = false) {
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
    return `<div class="line ${esc(e.level)}"><span class="time">${hh}:${mm}:${ss}</span>${esc(e.message)}</div>`;
  }).join('') || '<div class="hint">Журнал пуст.</div>';
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
    $$('#tabs button').forEach(b => b.classList.toggle('active', b === btn));
    $$('.page').forEach(p => { p.hidden = p.id !== `page-${btn.dataset.tab}`; });
    if (btn.dataset.tab === 'log') void loadEvents(false);
  }));
}

function init() {
  initTabs();
  $('#btn-add-provider').addEventListener('click', () => openAddModal());
  $('#btn-sync-presets').addEventListener('click', (e) => void syncPresets(e.currentTarget));
  $('#btn-modal-sync').addEventListener('click', (e) => void syncPresets(e.currentTarget));
  $('#btn-modal-cancel').addEventListener('click', closeAddModal);
  $('#btn-modal-save').addEventListener('click', () => void saveNewProvider());
  $('#add-modal').addEventListener('click', (e) => { if (e.target.id === 'add-modal') closeAddModal(); });

  $('#preset-select').addEventListener('change', (e) => {
    const opt = e.target.selectedOptions[0];
    if (!opt || !opt.value) return;
    // Подставляем name/base_url из каталога: пользователю остаётся вставить ключ.
    $('#new-name').value = opt.dataset.name || '';
    $('#new-base-url').value = opt.dataset.base || '';
    // `$('#new-id')`, а не `('#new-id')`: строка всегда истинна, `!` по ней даёт
    // `false`, и условие не выполнялось НИКОГДА — выбранный сервис не
    // подставлялся в поле id. Надо было спрашивать значение поля, а не строку.
    if (!$('#new-id').value) $('#new-id').value = opt.value;
    const docs = $('#new-docs');
    if (opt.dataset.docs) {
      docs.textContent = `Документация провайдера: ${opt.dataset.docs}`;
      docs.hidden = false;
    } else {
      docs.hidden = true;
    }
  });

  $('#btn-relay-test').addEventListener('click', () => void testRelay());
  $('#btn-relay-deploy').addEventListener('click', () => void deployRelay());
  $('#btn-relay-save').addEventListener('click', () => void saveRelayUrl());
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