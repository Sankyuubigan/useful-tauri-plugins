// Домен провайдеров дашборда: карточки, ключи, OAuth, модалки.
//
// Импортирует `refreshAll` из app.js (циклический импорт безопасен:
// это function-декларация в app.js, и к моменту любого вызова модуль
// уже вычислен). Остальное приходит из utils.js.

import {
  $, $$, state, api, toast, fail, busy, esc, plural,
  notifyHost, parseKeySpecs, keySpecsForPayload, keySpecsWithNameButNoSecret,
} from './utils.js';
// Циклический импорт app.js безопасен: там это function-декларации,
// hoisted и уже вычисленные к моменту любого вызова из этого файла.
import { refreshAll, refreshPresets, fillPresetSelect } from './app.js';

// ───────────────────────────── провайдеры ─────────────────────────────

function protocolBadge(p) {
  if (p.protocol === 'kiro') return '<span class="gw-badge gw-info">Kiro</span>';
  return '<span class="gw-badge">OpenAI</span>';
}

function authBlock(p) {
  // OAuth-привязка: секретов в ответе нет (только метаданные) — показываем
  // статус и кнопки входа/отвязки. API-ключи — как раньше.
  if (p.auth && p.auth.kind === 'o_auth') {
    const a = p.auth;
    const session = !a.session_present
      ? '<span class="gw-badge gw-err">сессия потеряна — войдите заново</span>'
      : (a.email ? `<span class="gw-hint">${esc(a.email)}</span>` : '<span class="gw-badge gw-ok">привязан</span>');
    const exp = a.expires_at_unix
      ? `<span class="gw-hint">токен до ${new Date(a.expires_at_unix * 1000).toLocaleString()}</span>`
      : '';
    return (
      `<div class="gw-kv"><span>Вход</span><b>Kiro OAuth${a.method ? ` (${esc(String(a.method))})` : ''}</b></div>` +
      `<div class="gw-row">${session} ${exp}` +
      `<span class="gw-spacer"></span>` +
      `<button class="gw-primary gw-btn-oauth-start" data-provider="${esc(p.id)}">Войти заново</button>` +
      `<button class="gw-ghost gw-btn-oauth-drop" data-provider="${esc(p.id)}">Отвязать</button>` +
      `</div>` +
      `<div class="gw-hint">Не вставляйте токен из 9Router: пройдите свой вход — иначе сломаете обе сессии.</div>`
    );
  }
  if (p.protocol === 'kiro') {
    return (
      `<div class="gw-row"><span class="gw-badge gw-warn">не привязан</span><span class="gw-spacer"></span>` +
      `<button class="gw-primary gw-btn-oauth-start" data-provider="${esc(p.id)}">Подключить Kiro (OAuth)</button></div>` +
      `<div class="gw-hint">Свой вход через AWS Builder ID: дашборд покажет код, подтвердите в браузере.</div>`
    );
  }
  return '';
}

/**
 * Ключ в карточке: заданное имя видно рядом с маской
 * («имя — AQ.Ab8…»), безымянный — просто маска.
 */
function keyLine(p, k, cd) {
  const label = k.name
    ? `<span class="gw-val"><span class="gw-keyname">${esc(k.name)}</span> — ${esc(k.masked)}</span>`
    : `<span class="gw-val">${esc(k.masked)}</span>`;
  const badges = [
    k.is_active ? '' : '<span class="gw-badge gw-err">выключен</span>',
    cd ? `<span class="gw-badge gw-warn">пауза ${cd} с</span>` : '',
  ].join(' ');
  return (
    `<div class="gw-key ${k.is_active ? '' : 'gw-off'}">` +
    label +
    badges +
    `<span class="gw-spacer"></span>` +
    `<button class="gw-ghost gw-btn-key-reset" data-provider="${esc(p.id)}" data-key="${esc(k.id)}"` +
    `${k.is_active && cd ? '' : ' hidden'}>Снять паузу</button>` +
    `<button class="gw-danger gw-btn-key-del" data-provider="${esc(p.id)}" data-key="${esc(k.id)}">Удалить</button>` +
    `</div>`
  );
}

/**
 * Строка статусов проверки ключей (вывод кнопки «Проверить ключи»).
 * Представлена отдельной функцией: её и вызывает checkKeys, и сам блок
 * всегда лежит на своём месте в карточке.
 */
function checkResultsBlock(p) {
  return `<div class="gw-keys gw-check-results" id="check-results-${esc(p.id)}" hidden></div>`;
}

function providerCard(p) {
  const status = p.routable
    ? '<span class="gw-badge gw-ok">используется</span>'
    : (!p.is_enabled
        ? '<span class="gw-badge">выключен</span>'
        : ((p.auth && p.auth.kind === 'o_auth')
            ? '<span class="gw-badge gw-warn">войдите через OAuth</span>'
            : (!p.keys.length
                ? '<span class="gw-badge gw-warn">нужен ключ</span>'
                : (p.keys.some(k => k.is_active)
                    ? '<span class="gw-badge gw-warn">нажмите «Обновить модели»</span>'
                    : '<span class="gw-badge gw-err">все ключи выключены</span>'))));
  const cooldownByKey = new Map(p.cooldowns.map(c => [c.key_id, c.remaining_sec]));

  const keys = p.keys.map(k => keyLine(p, k, cooldownByKey.get(k.id))).join('');

  const models = p.models.length
    ? `<div class="gw-models-list">${p.models.slice(0, 40).map(m => `<span>${esc(m)}</span>`).join('')}` +
      (p.models.length > 40 ? `<span>…и ещё ${p.models.length - 40}</span>` : '') + `</div>`
    : '';

  // Кнопка «Проверить ключи» — только для Gemini-провайдера (is_gemini
  // решает бэкенд, дашборд не дублирует эвристику по host'у). Без активных
  // ключей проверять нечего — кнопка заблокирована.
  const checkKeysBtn = p.is_gemini
    ? `<button class="gw-ghost gw-btn-keys-check" data-provider="${esc(p.id)}"` +
      `${p.keys.length ? '' : ' disabled'}>Проверить ключи</button>`
    : '';

  return (
    `<div class="gw-card ${p.is_enabled ? '' : 'gw-off'}">` +
    `<div class="gw-card-head">` +
    `<span class="gw-name">${esc(p.name)}</span>` +
    `<span class="gw-id">${esc(p.id)}</span>` +
    protocolBadge(p) +
    status +
    `<span class="gw-spacer"></span>` +
    `<label class="gw-inline"><input type="checkbox" class="gw-prov-toggle" data-provider="${esc(p.id)}"` +
    `${p.is_enabled ? ' checked' : ''}> включён</label>` +
    `<label class="gw-inline"><input type="checkbox" class="gw-prov-relay" data-provider="${esc(p.id)}"` +
    `${p.use_relay ? ' checked' : ''}> через релей</label>` +
    `</div>` +
    `<div class="gw-kv"><span>Адрес</span><b class="gw-break">${esc(p.base_url)}</b></div>` +
    authBlock(p) +
    `<div class="gw-kv"><span>Моделей</span><b>${p.models_count}</b></div>` +
    `<div class="gw-kv"><span>Ключей</span><b>${p.keys.length} (активных ${p.keys.filter(k => k.is_active).length})</b></div>` +
    models +
    `<div class="gw-row">` +
    `<button class="gw-primary gw-btn-models" data-provider="${esc(p.id)}">Обновить модели</button>` +
    `<button class="gw-ghost gw-btn-test" data-provider="${esc(p.id)}">Проверить связь</button>` +
    checkKeysBtn +
    `<button class="gw-ghost gw-btn-keys-add" data-provider="${esc(p.id)}">Добавить ключ</button>` +
    `<button class="gw-danger gw-btn-prov-del" data-provider="${esc(p.id)}">Удалить провайдера</button>` +
    `</div>` +
    `<div class="gw-keys">${keys || '<div class="gw-hint">Ключей нет — добавьте хотя бы один.</div>'}</div>` +
    checkResultsBlock(p) +
    `</div>`
  );
}

export function renderProviders() {
  const list = $('#providers-list');
  const problems = (state.config.problems || []).length
    ? `<div class="gw-card gw-problems"><h3>Проблемы конфигурации</h3>` +
      state.config.problems.map(p => `<div class="gw-hint">• ${esc(p)}</div>`).join('') + `</div>`
    : '';
  const body = state.config.providers.length
    ? state.config.providers.map(providerCard).join('')
    : `<div class="gw-empty">Провайдеров пока нет.<br>` +
      `Нажмите «Добавить провайдера», выберите сервис из каталога, вставьте API-ключ ` +
      `и нажмите «Обновить модели».</div>`;
  list.innerHTML = problems + body;
  bindProviderButtons();
}

function bindProviderButtons() {
  $$('.gw-prov-toggle').forEach(el => el.addEventListener('change', () =>
    patchProvider(el.dataset.provider, { is_enabled: el.checked })));
  $$('.gw-prov-relay').forEach(el => el.addEventListener('change', () =>
    patchProvider(el.dataset.provider, { use_relay: el.checked })));

  $$('.gw-btn-models').forEach(el => el.addEventListener('click', () => refreshModels(el)));
  $$('.gw-btn-test').forEach(el => el.addEventListener('click', () => testProvider(el)));
  $$('.gw-btn-keys-check').forEach(el => el.addEventListener('click', () => openCheckKeysModal(el.dataset.provider)));
  $$('.gw-btn-prov-del').forEach(el => el.addEventListener('click', () => deleteProvider(el)));
  $$('.gw-btn-key-del').forEach(el => el.addEventListener('click', () => deleteKey(el)));
  $$('.gw-btn-key-reset').forEach(el => el.addEventListener('click', () => resetKey(el)));
  $$('.gw-btn-oauth-start').forEach(el => el.addEventListener('click', () => startOAuth(el)));
  $$('.gw-btn-oauth-drop').forEach(el => el.addEventListener('click', () => dropOAuth(el)));
  $$('.gw-btn-keys-add').forEach(el => el.addEventListener('click', () => openKeysModal(el.dataset.provider)));
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

// ─────────────────────── проверка Gemini-ключей ───────────────────────

/**
 * Клик «Проверить ключи» открывает модалку выбора моделей (запрос 25.10.2026):
 * остаток лимитов зависит от модели, поэтому модель выбирает пользователь.
 * Предвыбран первый `gemini…`; пустой список моделей — подсказка про
 * «Обновить модели» и запасную модель бэкенда.
 */
let checkKeysFor = null;

function openCheckKeysModal(providerId) {
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) return;
  checkKeysFor = providerId;
  $('#checkkeys-provider').textContent = ` — ${p.name}`;
  const box = $('#checkkeys-models');
  if (p.models.length) {
    const first = p.models.find(m => String(m).startsWith('gemini')) || p.models[0];
    const shown = p.models.slice(0, 40);
    box.innerHTML = shown.map(m =>
      `<label class="gw-inline"><input type="checkbox" class="gw-checkkeys-model"` +
      ` value="${esc(m)}"${m === first ? ' checked' : ''}> ${esc(m)}</label>`
    ).join('') +
    (p.models.length > shown.length
      ? `<div class="gw-hint">…и ещё ${p.models.length - shown.length} — самые первые в списке</div>`
      : '');
  } else {
    box.innerHTML = '<div class="gw-hint">Список моделей пуст — закройте окно и нажмите ' +
      '«Обновить модели» в карточке. Иначе проверка пойдёт по запасной модели бэкенда.</div>';
  }
  $('#checkkeys-modal').hidden = false;
}

function closeCheckKeysModal() {
  $('#checkkeys-modal').hidden = true;
  checkKeysFor = null;
}

/** Бейдж вердикта и строка результата (общие для всех секций моделей). */
function checkRowHtml(x) {
  const badge = x.verdict === 'valid'
    ? '<span class="gw-badge gw-ok">валиден</span>'
    : x.verdict === 'invalid'
      ? '<span class="gw-badge gw-err">невалиден</span>'
      : x.verdict === 'valid_limited'
        ? '<span class="gw-badge gw-warn">в лимите</span>'
        : '<span class="gw-badge gw-warn">неизвестно</span>';
  const label = x.name ? `${esc(x.name)} — ${esc(x.masked)}` : esc(x.masked);
  return `<div class="gw-key"><span class="gw-val">${label}</span>${badge}</div>` +
    (x.detail ? `<div class="gw-hint gw-check-detail">${esc(x.detail)}</div>` : '');
}

/**
 * Проверить ключи по выбранным моделям (`:countTokens`, лимит генерации
 * не расходуется). Конфиг от проверки не меняется — refreshAll не нужен;
 * результат группируется по моделям в блоке `gw-check-results` карточки.
 */
async function runCheckKeys() {
  if (!checkKeysFor) return;
  const id = checkKeysFor;
  const models = $$('.gw-checkkeys-model').filter(el => el.checked).map(el => el.value);
  closeCheckKeysModal();

  const out = $(`#check-results-${CSS.escape(id)}`);
  const btn = document.querySelector(`.gw-btn-keys-check[data-provider="${CSS.escape(id)}"]`);
  busy(btn, true);
  if (out) {
    out.hidden = false;
    out.innerHTML = '<div class="gw-hint">проверяем ключи…</div>';
  }
  try {
    const body = models.length ? JSON.stringify({ models }) : undefined;
    const r = await api(`/api/providers/${encodeURIComponent(id)}/keys/check`, { method: 'POST', body });
    const nValid = r.results.filter(x => x.verdict === 'valid').length;
    const nLimited = r.results.filter(x => x.verdict === 'valid_limited').length;
    const nBad = r.results.filter(x => x.verdict === 'invalid').length;
    const nUnknown = r.results.filter(x => x.verdict === 'unknown').length;
    if (out) {
      // Секции по моделям в порядке ответа бэкенда.
      const byModel = new Map();
      r.results.forEach((x) => {
        if (!byModel.has(x.model)) byModel.set(x.model, []);
        byModel.get(x.model).push(x);
      });
      out.innerHTML = [...byModel.entries()].map(([model, rows]) =>
        `<div class="gw-check-model-title">${esc(model)}</div>` +
        rows.map(checkRowHtml).join('')
      ).join('') || '<div class="gw-hint">Проверять было нечего — активных ключей нет.</div>';
    }
    toast(
      `Проверка ключей (${r.models.length} ${plural(r.models.length, 'модель', 'модели', 'моделей')}): ` +
      `валидных ${nValid}, невалидных ${nBad}, в лимите ${nLimited}, неизвестно ${nUnknown}` +
      `${r.via_relay ? ' (через релей)' : ''}`,
      (nValid + nLimited) === r.results.length ? 'ok' : 'err',
    );
  } catch (e) {
    fail('Проверка ключей не удалась', e);
    if (out) { out.innerHTML = ''; out.hidden = true; }
  } finally { busy(btn, false); }
}

// ─────────────────────── OAuth Kiro ───────────────────────

let oauthTimer = null;

function stopOAuthPolling() {
  if (oauthTimer) { clearInterval(oauthTimer); oauthTimer = null; }
  $('#oauth-modal').hidden = true;
}

async function startOAuth(btn) {
  const id = btn.dataset.provider;
  busy(btn, true);
  try {
    const r = await api(`/api/providers/${encodeURIComponent(id)}/oauth/start`, {
      method: 'POST',
      body: JSON.stringify({ region: 'us-east-1' }),
    });
    $('#oauth-user-code').textContent = r.user_code;
    $('#oauth-uri').textContent = r.verification_uri_complete || r.verification_uri;
    $('#oauth-open').href = r.verification_uri_complete || r.verification_uri;
    $('#oauth-status').textContent = 'Ожидание подтверждения в браузере…';
    $('#oauth-modal').hidden = false;
    stopOAuthPolling();
    // Поллинг раз в 5 секунд: чаще AWS просит замедлиться (slow_down).
    oauthTimer = setInterval(() => void pollOAuth(id, r.session_id), 5000);
  } catch (e) { fail('Не удалось начать вход', e); }
  finally { busy(btn, false); }
}

async function pollOAuth(id, sessionId) {
  try {
    const r = await api(`/api/providers/${encodeURIComponent(id)}/oauth/poll`, {
      method: 'POST',
      body: JSON.stringify({ session_id: sessionId }),
    });
    if (r.status === 'done') {
      stopOAuthPolling();
      toast('Kiro подключён. Нажмите «Обновить модели».');
      await refreshAll();
      notifyHost('provider-patched');
    }
  } catch (e) {
    stopOAuthPolling();
    fail('Вход не удался', e);
  }
}

async function dropOAuth(btn) {
  const id = btn.dataset.provider;
  if (!window.confirm(`Отвязать Kiro-сессию провайдера «${id}»? Токены будут удалены.`)) return;
  busy(btn, true);
  try {
    await api(`/api/providers/${encodeURIComponent(id)}/oauth/disconnect`, { method: 'POST' });
    toast('Сессия удалена');
    await refreshAll();
    notifyHost('provider-patched');
  } catch (e) { fail('Не удалось отвязать', e); }
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

// ─────────────────────── модалка «Добавить ключ» ───────────────────────

/**
 * Массовое добавление ключей: в textarea — по ключу на строку, формат
 * «имя|секрет» или просто «секрет». Раньше здесь был window.prompt(1 ключ
 * за клик) — для пакетного переноса из AI Studio это было не выражаемым.
 */
let keysModalFor = null;

function openKeysModal(providerId) {
  keysModalFor = providerId;
  $('#keys-input').value = '';
  $('#keys-modal').hidden = false;
  $('#keys-input').focus();
}

function closeKeysModal() {
  $('#keys-modal').hidden = true;
  keysModalFor = null;
}

async function saveKeys() {
  if (!keysModalFor) return;
  const btn = $('#btn-keys-save');
  const parsed = parseKeySpecs($('#keys-input').value);
  if (!parsed.length) {
    toast('Впишите хотя бы один ключ (формат: имя|ключ или просто ключ)', 'err');
    return;
  }
  // Строка с именем, но без секрета — ошибка валидации, а не молчаливое
  // выбрасывание: пользователь должен увидеть потерю и исправить строку.
  const broken = keySpecsWithNameButNoSecret(parsed);
  if (broken.length) {
    const list = broken.map(b => `«${b}»`).join(', ');
    toast(`В строках ${list} нет секрета после | — исправьте или уберите имя`, 'err');
    return;
  }
  busy(btn, true);
  try {
    const r = await api(
      `/api/providers/${encodeURIComponent(keysModalFor)}/keys`,
      { method: 'POST', body: JSON.stringify({ keys: keySpecsForPayload(parsed) }) },
    );
    closeKeysModal();
    const added = r.added_keys.length;
    toast(`Добавлено ${added} ${plural(added, 'ключ', 'ключа', 'ключей')}` +
      (added ? `: ${r.added_keys.join(', ')}` : ''));
    await refreshAll();
    notifyHost('keys-added');
  } catch (e) { fail('Не удалось добавить ключи', e); }
  finally { busy(btn, false); }
}

// ─────────────── модалка добавления провайдера ───────────────

function openAddModal() {
  $('#new-id').value = '';
  $('#new-name').value = '';
  $('#new-base-url').value = '';
  $('#new-keys').value = '';
  $('#new-docs').hidden = true;
  $('#preset-search').value = '';
  fillPresetSelect();
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
  const parsed = parseKeySpecs($('#new-keys').value);
  const broken = keySpecsWithNameButNoSecret(parsed);
  if (broken.length) {
    const list = broken.map(b => `«${b}»`).join(', ');
    toast(`В строках ${list} нет секрета после | — исправьте или уберите имя`, 'err');
    return;
  }
  const body = {
    preset_id: $('#preset-select').value || null,
    id: $('#new-id').value.trim() || null,
    name: $('#new-name').value.trim() || null,
    base_url: $('#new-base-url').value.trim() || null,
    keys: keySpecsForPayload(parsed),
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

// ─────────────── статические привязки домена провайдеров ───────────────

/** Вызывается один раз из init() app.js: кнопки модалок и поиск каталога. */
export function bindProviderExtras() {
  $('#btn-add-provider').addEventListener('click', () => openAddModal());
  $('#btn-modal-cancel').addEventListener('click', closeAddModal);
  $('#btn-modal-save').addEventListener('click', () => void saveNewProvider());
  $('#add-modal').addEventListener('click', (e) => { if (e.target.id === 'add-modal') closeAddModal(); });

  $('#preset-search').addEventListener('input', () => fillPresetSelect());
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

  $('#btn-oauth-cancel').addEventListener('click', stopOAuthPolling);
  $('#oauth-modal').addEventListener('click', (e) => { if (e.target.id === 'oauth-modal') stopOAuthPolling(); });

  $('#btn-keys-cancel').addEventListener('click', closeKeysModal);
  $('#btn-keys-save').addEventListener('click', () => void saveKeys());
  $('#keys-modal').addEventListener('click', (e) => { if (e.target.id === 'keys-modal') closeKeysModal(); });

  $('#btn-checkkeys-cancel').addEventListener('click', closeCheckKeysModal);
  $('#btn-checkkeys-run').addEventListener('click', () => void runCheckKeys());
  $('#checkkeys-modal').addEventListener('click', (e) => { if (e.target.id === 'checkkeys-modal') closeCheckKeysModal(); });
}
