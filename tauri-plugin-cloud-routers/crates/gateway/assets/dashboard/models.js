// Домен вкладки «Модели» дашборда: выбор моделей, которые видит чат/IDE.
//
// Разделение файлов — «по домену» (как providers.js/combos.js). Единственный
// источник правды — REST API шлюза: выбор хранится в ProviderConfig.hidden_models
// (чёрный список id моделей), записывается PATCH-ем /api/providers/:id и
// применяется на бэкенде В ЕДИНСТВЕННОМ построителе списка list_combos
// (core/src/models.rs) — через него идут /v1/models, /api/combos и файловый
// фолбэк хоста. Дропдаун чата обновляется живо: notifyHost → хост перечитывает
// /api/combos.
//
// Семантика: галочка = модель видна в списке чата/IDE; скрытая модель исчезает
// ТОЛЬКО из списка, маршрутизация не трогается (resolve_combo конфиг читает
// напрямую) — старые сессии и IDE с прямым id продолжают работать. Пустой
// hidden_models = показывать все (новые модели после «Обновить модели» видны
// сразу).

import {
  $, $$, state, api, esc, plural, fail, busy, notifyHost,
} from './utils.js';
// Циклический импорт безопасен: refreshAll — function-декларация в app.js,
// к моменту любого вызова модуль с ней уже вычислен (паттерн combos.js).
import { refreshAll } from './app.js';

// Дебаунс PATCH на провайдера: серия кликов по галочкам = один PATCH и одна
// запись конфига на диск. Ключ Map — id провайдера.
const SAVE_DEBOUNCE_MS = 500;
const saveTimers = new Map();

// ───────────────────────────── рендер вкладки ─────────────────────────────

/** Перерисовать вкладку «Модели». Вызывается из refreshAll (app.js). */
export function renderModels() {
  renderModelsList();
}

function renderModelsList() {
  const list = $('#models-list');
  const ready = state.config.providers.filter(p => p.models_count > 0);
  if (!ready.length) {
    list.innerHTML = '<div class="gw-empty">Моделей пока нет.<br>' +
      'Добавьте провайдера и нажмите «Обновить модели» — список опрашивается у провайдера, а не берётся из каталога.</div>';
    applyModelsFilter();
    return;
  }
  list.innerHTML = ready.map(providerCard).join('');
  applyModelsFilter();
}

/** Карточка провайдера: шапка с бейджем «N из M в чате» + сетка чекбоксов. */
function providerCard(p) {
  const hidden = new Set(p.hidden_models || []);
  const total = p.models.length;
  const visible = p.models.filter(m => !hidden.has(m)).length;
  const badge = hidden.size
    ? `в чате ${visible} из ${total}`
    : `${total} ${plural(total, 'модель', 'модели', 'моделей')} в чате`;
  const items = p.models.map(m =>
    `<label class="gw-inline gw-model-item" data-model="${esc(m)}" title="${esc(m)}">` +
    `<input type="checkbox" data-model-toggle data-provider="${esc(p.id)}" data-model="${esc(m)}"${hidden.has(m) ? '' : ' checked'}>` +
    `<span>${esc(m)}</span>` +
    `</label>`
  ).join('');
  return (
    `<div class="gw-card" data-models-provider="${esc(p.id)}">` +
    `<div class="gw-card-head">` +
    `<span class="gw-name">${esc(p.name)}</span>` +
    `<span class="gw-id">${esc(p.id)}</span>` +
    `<span class="gw-badge${hidden.size ? ' gw-warn' : ''}" data-models-badge>${badge}</span>` +
    `<span class="gw-spacer"></span>` +
    `<button class="gw-ghost" data-models-showall="${esc(p.id)}">Показать все</button>` +
    `<button class="gw-ghost" data-models-hideall="${esc(p.id)}">Скрыть все</button>` +
    `</div>` +
    `<div class="gw-kv"><span>В чате как</span><b class="gw-mono">${esc(p.id)}/&lt;модель&gt;</b></div>` +
    `<div class="gw-model-items">${items}</div>` +
    `</div>`
  );
}

// ─────────────────────────────── поиск ───────────────────────────────

/** Клиентский фильтр + счётчик: запрос бьётся на слова, каждое слово обязано
 *  найтись в «id модели + id/имя провайдера» без учёта регистра (AND-токены).
 *  Цельный поиск по подстроке здесь сломан по построению: «gemini flash» как
 *  подстрока не встречается в «gemini-2.5-flash» — ни одна модель не
 *  находилась (репорт юзера 08.11.26). Карточка провайдера скрывается целиком,
 *  если ни одна модель не совпала; без запроса — счётчик «N моделей у M
 *  провайдеров». */
function applyModelsFilter() {
  const input = $('#models-search');
  const hint = $('#models-hint');
  if (!input || !hint) return;
  // Слова запроса — независимые требования: «gemini flash» = есть «gemini» И
  // есть «flash». Пустой запрос → tokens = [] → every = истина (все видны).
  const tokens = (input.value || '').trim().toLowerCase().split(/\s+/).filter(Boolean);
  const cards = $$('#models-list .gw-card[data-models-provider]');
  if (!cards.length) { hint.textContent = ''; return; }
  let shown = 0;
  let total = 0;
  cards.forEach((card) => {
    const name = (card.querySelector('.gw-name') || {}).textContent || '';
    const provHay = `${card.dataset.modelsProvider} ${name}`.toLowerCase();
    let inCard = 0;
    card.querySelectorAll('.gw-model-item').forEach((item) => {
      total += 1;
      const hay = `${item.dataset.model} ${provHay}`.toLowerCase();
      const hit = tokens.every(t => hay.includes(t));
      item.classList.toggle('gw-filter-hidden', !hit);
      if (hit) { inCard += 1; }
    });
    card.hidden = tokens.length ? inCard === 0 : false;
    shown += inCard;
  });
  hint.textContent = tokens.length
    ? `показано ${shown} из ${total}`
    : `${total} ${plural(total, 'модель', 'модели', 'моделей')} у ${cards.length} провайдеров`;
}

// ─────────────────────────── изменения выбора ───────────────────────────

/** Клик по галочке: пересчитать hidden_models провайдера из DOM карточки.
 *  hidden = обнаруженные модели минус отмеченные: источник id — p.models, id
 *  вне списка структурно невозможны (чекбоксы рисуются из p.models). */
function onModelToggle(input) {
  const providerId = input.dataset.provider;
  const p = state.config.providers.find(x => x.id === providerId);
  const card = input.closest('.gw-card');
  if (!p || !card) return;
  const checked = new Set(
    [...card.querySelectorAll('.gw-model-item input')]
      .filter(x => x.checked)
      .map(x => x.dataset.model),
  );
  p.hidden_models = p.models.filter(m => !checked.has(m));
  updateModelsBadge(providerId);
  scheduleSave(providerId);
  // Фильтр не пере-применяется: текст модели не менялся, совпадение с поиском
  // не зависит от галочки.
}

/** «Показать все»/«Скрыть все»: одно атомарное действие с немедленным
 *  сохранением — отложенный PATCH отменяется, чтобы выбор не потерялся, если
 *  страницу закроют сразу после клика. */
async function bulkVisibility(providerId, hideAll, btn) {
  busy(btn, true);
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) { busy(btn, false); return; }
  clearTimeout(saveTimers.get(providerId));
  saveTimers.delete(providerId);
  p.hidden_models = hideAll ? [...p.models] : [];
  const card = $(`#models-list .gw-card[data-models-provider="${CSS.escape(providerId)}"]`);
  if (card) {
    card.querySelectorAll('.gw-model-item input').forEach(x => { x.checked = !hideAll; });
  }
  updateModelsBadge(providerId);
  try {
    await saveHiddenNow(providerId);
  } finally { busy(btn, false); }
}

// ─────────────────────────────── сохранение ───────────────────────────────

function scheduleSave(providerId) {
  clearTimeout(saveTimers.get(providerId));
  saveTimers.set(providerId, setTimeout(() => {
    saveTimers.delete(providerId);
    void saveHiddenNow(providerId);
  }, SAVE_DEBOUNCE_MS));
}

/** PATCH /api/providers/:id с ПОЛНЫМ списком hidden_models провайдера.
 *  Семантика PATCH — «замена списка целиком» (admin.rs ProviderPatch), поэтому
 *  последний PATCH несёт полное состояние: гонка «клик во время полёта»
 *  разрешается сама — клик обновляет state до отправки; из ответа DTO берутся
 *  только model-поля (models/models_count), локальный hidden_models не
 *  перезаписывается (серверная нормализация trim/dedup для id из p.models
 *  тождественна). */
async function saveHiddenNow(providerId) {
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) return;
  try {
    const dto = await api(`/api/providers/${encodeURIComponent(providerId)}`, {
      method: 'PATCH',
      body: JSON.stringify({ hidden_models: p.hidden_models }),
    });
    // Из ответа берём ТОЛЬКО model-поля: локальный hidden_models — истина
    // клиента (серверная нормализация trim/dedup для id из p.models
    // тождественна), а keys и прочие поля карточек провайдеров вкладка не
    // редактирует.
    p.models = dto.models;
    p.models_count = dto.models_count;
    updateModelsBadge(providerId);
    notifyHost('models-visibility-changed');
  } catch (e) {
    // Честность: молчаливой потери галочек не бывает — тост с причиной и
    // перерисовка вкладки из реального состояния сервера.
    fail('Не удалось сохранить выбор моделей', e);
    await refreshAll();
  }
}

// ─────────────────────────────── бейдж ───────────────────────────────

/** Обновить бейдж «N из M в чате» без полного рендера: полный рендер стирал бы
 *  курсор поиска и был бы заметен глазу на каждый клик. */
function updateModelsBadge(providerId) {
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) return;
  const card = $(`#models-list .gw-card[data-models-provider="${CSS.escape(providerId)}"]`);
  if (!card) return;
  const badge = card.querySelector('[data-models-badge]');
  if (!badge) return;
  const hidden = new Set(p.hidden_models || []);
  const total = p.models.length;
  const visible = p.models.filter(m => !hidden.has(m)).length;
  badge.textContent = hidden.size
    ? `в чате ${visible} из ${total}`
    : `${total} ${plural(total, 'модель', 'модели', 'моделей')} в чате`;
  badge.classList.toggle('gw-warn', hidden.size > 0);
}

// ─────────────────────────────── биндинг ───────────────────────────────

/** Вызывается один раз из init() app.js: поиск + делегирование на контейнер
 *  списка. Делегирование — потому что renderModels перерисовывает innerHTML:
 *  прямые слушатели на чекбоксах погибли бы при первом же рендере. */
export function bindModelExtras() {
  $('#models-search').addEventListener('input', applyModelsFilter);
  const list = $('#models-list');
  // change ВСПЛЫВАЕТ (в отличие от focus/blur), поэтому делегированный change
  // честен; для булк-кнопок — click.
  list.addEventListener('change', (e) => {
    const t = e.target;
    if (t instanceof HTMLInputElement && t.dataset.modelToggle !== undefined) {
      onModelToggle(t);
    }
  });
  list.addEventListener('click', (e) => {
    const show = e.target.closest('[data-models-showall]');
    const hide = e.target.closest('[data-models-hideall]');
    if (show) void bulkVisibility(show.dataset.modelsShowall, false, show);
    else if (hide) void bulkVisibility(hide.dataset.modelsHideall, true, hide);
  });
}
