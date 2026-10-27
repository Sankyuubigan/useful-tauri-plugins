// Домен комбо-связок дашборда: список, редактор, сохранение/удаление.
//
// Разделение файлов — «по домену» (как providers.js): связки — свой домен со
// своей модалкой. Единственный источник правды — REST API шлюза
// (/api/combos/config*); страница ничего не запоминает между перезагрузками.
// После сохранения хост уведомляется postMessage (notifyHost) — выпадающий
// список моделей чата перечитает комбо без переоткрытия настроек.

import {
  $, $$, state, api, toast, fail, busy, esc, plural, notifyHost,
} from './utils.js';
// Циклический импорт безопасен: refreshAll — function-декларация в app.js,
// к моменту любого вызова модуль с ней уже вычислен.
import { refreshAll } from './app.js';

// ───────────────────────────── список связок ─────────────────────────────

/** Перечитать связки и отрисовать вкладку. Вызывается из refreshAll. */
export async function renderCombos() {
  const list = $('#combos-list');
  let combos;
  try {
    combos = (await api('/api/combos/config')).combos || [];
  } catch (e) {
    fail('Не удалось загрузить связки', e);
    return;
  }
  state.userCombos = combos;
  const hint = $('#combos-hint');
  if (hint) {
    hint.textContent = combos.length
      ? `${combos.length} ${plural(combos.length, 'связка', 'связки', 'связок')}`
      : 'связок нет';
  }
  if (!combos.length) {
    list.innerHTML = '<div class="gw-empty">Связок пока нет.<br>' +
      'Нажмите «Добавить связку», выберите модели провайдеров и их ключи — ' +
      'в чате связка появится как отдельная модель.</div>';
    return;
  }
  list.innerHTML = combos.map(comboCard).join('');
  $$('#combos-list [data-combo-edit]').forEach(btn =>
    btn.addEventListener('click', () => openComboModal(btn.dataset.comboEdit)));
  $$('#combos-list [data-combo-del]').forEach(btn =>
    btn.addEventListener('click', () => deleteCombo(btn.dataset.comboDel, btn)));
}

/** Карточка связки: имя + цепочка участников с пометкой ключей. */
function comboCard(c) {
  const chain = c.members.map((m, i) => {
    const p = state.config.providers.find(x => x.id === m.provider_id);
    const pName = p ? p.name : m.provider_id;
    // Пометка ключей: «все активные» либо перечисление имён/масок по id.
    const keysNote = !m.key_ids
      ? 'все активные'
      : (m.key_ids.length
          ? m.key_ids.map((id) => {
              const k = p && p.keys.find(x => x.id === id);
              return k ? (k.name || k.masked) : `${id} (удалён)`;
            }).join(', ')
          : '— пусто (участник не примет запрос)');
    const step = `${i + 1}. `;
    const arrow = i === 0 ? '' : '<span class="gw-combo-arrow">→ </span>';
    return `<div class="gw-combo-step">${arrow}<span class="gw-combo-num">${step}</span>` +
      `<span class="gw-mono">${esc(pName)} / ${esc(m.model_id)}</span>` +
      `<span class="gw-hint"> · ключи: ${esc(keysNote)}</span></div>`;
  }).join('');
  return (
    `<div class="gw-card">` +
    `<div class="gw-card-head">` +
    `<span class="gw-name">${esc(c.name)}</span>` +
    `<span class="gw-id">в чате: ${esc(c.name)}</span>` +
    `<span class="gw-badge">${c.members.length} ${plural(c.members.length, 'участник', 'участника', 'участников')}</span>` +
    `<span class="gw-spacer"></span>` +
    `<button class="gw-ghost" data-combo-edit="${esc(c.name)}">Изменить</button>` +
    `<button class="gw-danger" data-combo-del="${esc(c.name)}">Удалить</button>` +
    `</div>` +
    `<div class="gw-combo-chain">${chain}</div>` +
    `</div>`
  );
}

// ───────────────────────────── редактор связки ─────────────────────────────

/** Черновик участников модалки: [{provider_id, model_id, key_ids}]. */
let draftMembers = [];
/** Имя редактируемой связки (null — создание новой). */
let editingName = null;

function openComboModal(name = null) {
  editingName = name;
  const combo = name ? state.userCombos.find(c => c.name === name) : null;
  $('#combo-modal-title').textContent = combo ? `Изменить связку «${name}»` : 'Новая связка';
  $('#combo-name').value = combo ? combo.name : '';
  // Из карточек приходят полные id участников (provider/model) — разбираем
  // обратно в черновик. Слэша в id провайдера не бывает (валидация конфига).
  draftMembers = combo
    ? combo.members.map(m => ({
        provider_id: m.provider_id,
        model_id: m.model_id,
        key_ids: m.key_ids ? [...m.key_ids] : null,
      }))
    : [];
  if (!draftMembers.length) draftMembers.push(blankMember());
  $('#combo-modal').hidden = false;
  renderDraft();
}

function blankMember() {
  return { provider_id: '', model_id: '', key_ids: null };
}

function closeComboModal() {
  $('#combo-modal').hidden = true;
  draftMembers = [];
  editingName = null;
}

/** Перерисовать участников в модалке из черновика. */
function renderDraft() {
  const box = $('#combo-members');
  box.innerHTML = draftMembers.map((m, i) => {
    const provs = state.config.providers
      .map(p => `<option value="${esc(p.id)}"${p.id === m.provider_id ? ' selected' : ''}>` +
        `${esc(p.name)} (${esc(p.id)})</option>`)
      .join('');
    const sel = state.config.providers.find(p => p.id === m.provider_id);
    const models = sel
      ? sel.models.map(mid =>
          `<option value="${esc(mid)}"${mid === m.model_id ? ' selected' : ''}>${esc(mid)}</option>`).join('')
      : '';
    const keys = sel
      ? sel.keys.map(k => {
          const checked = m.key_ids ? m.key_ids.includes(k.id) : false;
          const label = k.name ? `${esc(k.name)} — ${esc(k.masked)}` : esc(k.masked);
          const off = k.is_active ? '' : ' gw-hint';
          return `<label class="gw-inline${off}">` +
            `<input type="checkbox" class="gw-combo-key" data-idx="${i}" value="${esc(k.id)}"` +
            `${checked ? ' checked' : ''}${k.is_active ? '' : ' disabled'}> ${label}` +
            `${k.is_active ? '' : ' (выключен)'}</label>`;
        }).join('')
      : '';
    const keysState = m.key_ids === null
      ? '<div class="gw-hint gw-combo-keys-note">все активные ключи</div>'
      : '<div class="gw-hint gw-combo-keys-note">только отмеченные (порядок отметок = порядок попыток)</div>';
    return (
      `<div class="gw-card gw-combo-member">` +
      `<div class="gw-row">` +
      `<span class="gw-combo-num">${i + 1}</span>` +
      `<label class="gw-inline">провайдер ` +
      `<select class="gw-combo-prov" data-idx="${i}">` +
      `<option value=""${sel ? '' : ' selected'}>— выберите —</option>${provs}</select></label>` +
      `<label class="gw-inline">модель ` +
      `<select class="gw-combo-model" data-idx="${i}"${sel ? '' : ' disabled'}>` +
      `<option value=""${m.model_id ? '' : ' selected'}>— выберите —</option>${models}</select></label>` +
      `<span class="gw-spacer"></span>` +
      `<button class="gw-ghost" data-move-up="${i}"${i === 0 ? ' disabled' : ''}>↑</button>` +
      `<button class="gw-ghost" data-move-down="${i}"${i === draftMembers.length - 1 ? ' disabled' : ''}>↓</button>` +
      `<button class="gw-danger" data-remove="${i}">✕</button>` +
      `</div>` +
      `<div class="gw-combo-keys">${keys || '<div class="gw-hint">У провайдера нет ключей.</div>'}</div>` +
      keysState +
      `</div>`
    );
  }).join('');
  bindDraft();
}

function bindDraft() {
  $$('#combo-members .gw-combo-prov').forEach(el => el.addEventListener('change', () => {
    const m = draftMembers[Number(el.dataset.idx)];
    m.provider_id = el.value;
    m.model_id = '';
    m.key_ids = null;
    renderDraft();
  }));
  $$('#combo-members .gw-combo-model').forEach(el => el.addEventListener('change', () => {
    draftMembers[Number(el.dataset.idx)].model_id = el.value;
  }));
  $$('#combo-members .gw-combo-key').forEach(el => el.addEventListener('change', () => {
    const i = Number(el.dataset.idx);
    const m = draftMembers[i];
    // Снятие последней отметки возвращает «все активные» (key_ids = null) —
    // иначе участник с пустым списком структурно отклонялся бы бэкендом.
    const checked = $$('#combo-members .gw-combo-key').filter(x => Number(x.dataset.idx) === i && x.checked);
    m.key_ids = checked.length ? checked.map(x => x.value) : null;
    renderDraft();
  }));
  $$('#combo-members [data-move-up]').forEach(el => el.addEventListener('click', () => {
    const i = Number(el.dataset.moveUp);
    [draftMembers[i - 1], draftMembers[i]] = [draftMembers[i], draftMembers[i - 1]];
    renderDraft();
  }));
  $$('#combo-members [data-move-down]').forEach(el => el.addEventListener('click', () => {
    const i = Number(el.dataset.moveDown);
    [draftMembers[i], draftMembers[i + 1]] = [draftMembers[i + 1], draftMembers[i]];
    renderDraft();
  }));
  $$('#combo-members [data-remove]').forEach(el => el.addEventListener('click', () => {
    draftMembers.splice(Number(el.dataset.remove), 1);
    if (!draftMembers.length) draftMembers.push(blankMember());
    renderDraft();
  }));
}

/** Клиентская валидация = зеркальная копия Rust-валидации конфига: не пустое
 * имя, без «/», уникальное, у каждого участника выбраны провайдер и модель. */
function validateDraft() {
  const name = $('#combo-name').value.trim();
  if (!name) return 'Введите имя связки';
  if (name.includes('/')) return 'Имя не должно содержать «/»';
  const sameName = state.userCombos.find(c => c.name === name);
  if (sameName && sameName.name !== editingName) return `Связка «${name}» уже есть`;
  for (let i = 0; i < draftMembers.length; i++) {
    const m = draftMembers[i];
    if (!m.provider_id) return `Участник ${i + 1}: выберите провайдера`;
    if (!m.model_id) return `Участник ${i + 1}: выберите модель`;
  }
  return null;
}

async function saveCombo() {
  const problem = validateDraft();
  if (problem) {
    toast(problem, 'err');
    return;
  }
  const btn = $('#btn-combo-save');
  const name = $('#combo-name').value.trim();
  const body = JSON.stringify({
    members: draftMembers.map(m => ({
      provider_id: m.provider_id,
      model_id: m.model_id,
      key_ids: m.key_ids,
    })),
  });
  busy(btn, true);
  try {
    await api(`/api/combos/config/${encodeURIComponent(name)}`, { method: 'PUT', body });
    closeComboModal();
    toast(`Связка «${name}» сохранена`);
    await refreshAll();
    notifyHost('combos-changed');
  } catch (e) {
    fail('Не удалось сохранить связку', e);
  } finally { busy(btn, false); }
}

async function deleteCombo(name, btn) {
  if (!window.confirm(`Удалить связку «${name}»? Из чата исчезнет модель с этим именем.`)) return;
  busy(btn, true);
  try {
    await api(`/api/combos/config/${encodeURIComponent(name)}`, { method: 'DELETE' });
    toast('Связка удалена');
    await refreshAll();
    notifyHost('combos-changed');
  } catch (e) { fail('Не удалось удалить связку', e); }
  finally { busy(btn, false); }
}

/** Вызывается один раз из init() app.js: статические кнопки домена связок. */
export function bindComboExtras() {
  $('#btn-add-combo').addEventListener('click', () => openComboModal(null));
  $('#btn-combo-add-member').addEventListener('click', () => {
    draftMembers.push(blankMember());
    renderDraft();
  });
  $('#btn-combo-cancel').addEventListener('click', closeComboModal);
  $('#btn-combo-save').addEventListener('click', () => void saveCombo());
  $('#combo-modal').addEventListener('click', (e) => { if (e.target.id === 'combo-modal') closeComboModal(); });
}
