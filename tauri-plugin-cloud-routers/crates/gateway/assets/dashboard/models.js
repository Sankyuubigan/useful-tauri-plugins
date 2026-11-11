// Домен вкладки «Модели» дашборда: выбор моделей, которые видит чат/IDE.
//
// Разделение файлов — «по домену» (как providers.js/combos.js). Единственный
// источник правды — REST API шлюза: выбор хранится в ProviderConfig.visible_models
// (белый список id моделей — «что показывать»), записывается PATCH-ем
// /api/providers/:id и применяется на бэкенде В ЕДИНСТВЕННОМ построителе списка
// list_combos (core/src/models.rs) — через него идут /v1/models, /api/combos и
// файловый фолбэк хоста. Дропдаун чата обновляется живо: notifyHost → хост
// перечитывает /api/combos.
//
// Семантика: галочка = модель видна в списке чата/IDE; неотмеченная модель
// исчезает ТОЛЬКО из списка, маршрутизация не трогается (resolve_combo конфиг
// читает напрямую) — старые сессии и IDE с прямым id продолжают работать.
// Пустой visible_models = показывать ничего (едино правило без веток). Инвариант
// «Обновить модели» не раздаёт видимость (баг 13→24) держится моделью данных:
// новый id апстрима структурно неотмечен.
//
// Stale-выбор («пометить нерабочими»): модель, отмеченная юзером, но исчезнувшая
// у апстрима, ОСТАЁТСЯ отмеченной в visible_models (чекбокса у неё нет — список
// рисуется из p.models); когда модель вернётся, она снова видима сама.

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

/** Allowlist провайдера как Set: id, отмеченные для чата. Пусто = не показывать
 *  ничего. Id вне p.models — stale-выбор (исчез у апстрима, остался отмеченным). */
function visibleSet(p) {
  return new Set(p.visible_models || []);
}

/** Карточка провайдера: шапка с бейджем «N из M в чате» + сетка чекбоксов. */
function providerCard(p) {
  const chosen = visibleSet(p);
  const total = p.models.length;
  const shown = p.models.filter(m => chosen.has(m)).length;
  const badge = shown === total
    ? `${total} ${plural(total, 'модель', 'модели', 'моделей')} в чате`
    : `в чате ${shown} из ${total}`;
  // warn вычисляется ЗАРАНЕЕ (а не тернарником внутри class="..."): сканер
  // namespace (server.rs) токенизирует класс-литералы по пробелам, голое слово
  // `total` внутри атрибута — ложный «нарушитель gw-».
  const warn = shown === total ? '' : ' gw-warn';
  const items = p.models.map(m =>
    `<label class="gw-inline gw-model-item" data-model="${esc(m)}" title="${esc(m)}">` +
    `<input type="checkbox" data-model-toggle data-provider="${esc(p.id)}" data-model="${esc(m)}"${chosen.has(m) ? ' checked' : ''}>` +
    `<span>${esc(m)}</span>` +
    `</label>`
  ).join('');
  return (
    `<div class="gw-card" data-models-provider="${esc(p.id)}">` +
    `<div class="gw-card-head">` +
    `<span class="gw-name">${esc(p.name)}</span>` +
    `<span class="gw-id">${esc(p.id)}</span>` +
    `<span class="gw-badge${warn}" data-models-badge>${badge}</span>` +
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

/** Отмеченное в allowlist вне p.models — stale-выбор (исчез у апстрима,
 *  остался отмеченным): при пересчётах он сохраняется. */
function staleChosen(p) {
  const models = new Set(p.models);
  return (p.visible_models || []).filter(m => !models.has(m));
}

/** Клик по галочке: пересчитать visible_models провайдера из DOM карточки.
 *  Галочки — union DOM-состояния и stale-выбора: id, отмеченные ранее, но
 *  исчезнувшие у апстрима (чекбокса нет — список рисуется из p.models), не
 *  теряются, иначе выбор молча обнулялся бы при первом же churn'е апстрима. */
function onModelToggle(input) {
  const providerId = input.dataset.provider;
  const p = state.config.providers.find(x => x.id === providerId);
  const card = input.closest('.gw-card');
  if (!p || !card) return;
  const checked = [
    ...card.querySelectorAll('.gw-model-item input')
      .filter(x => x.checked)
      .map(x => x.dataset.model),
    ...staleChosen(p),
  ];
  p.visible_models = [...new Set(checked)];
  updateModelsBadge(providerId);
  scheduleSave(providerId);
  // Фильтр не пере-применяется: текст модели не менялся, совпадение с поиском
  // не зависит от галочки.
}

/** «Показать все»/«Скрыть все»: одно атомарное действие с немедленным
 *  сохранением — отложенный PATCH отменяется, чтобы выбор не потерялся, если
 *  страницу закроют сразу после клика. Stale-выбор переживает оба действия:
 *  «показать все» его не трогает (он и так выбран), «скрыть все» прячет только
 *  то, что есть у апстрима сейчас. */
async function bulkVisibility(providerId, hideAll, btn) {
  busy(btn, true);
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) { busy(btn, false); return; }
  clearTimeout(saveTimers.get(providerId));
  saveTimers.delete(providerId);
  const stale = staleChosen(p);
  p.visible_models = hideAll
    ? stale
    : [...new Set([...p.models, ...stale])];
  const card = $(`#models-list .gw-card[data-models-provider="${CSS.escape(providerId)}"]`);
  if (card) {
    card.querySelectorAll('.gw-model-item input').forEach(x => { x.checked = !hideAll; });
  }
  updateModelsBadge(providerId);
  try {
    await saveVisibleNow(providerId);
  } finally { busy(btn, false); }
}

// ─────────────────────────────── сохранение ───────────────────────────────

function scheduleSave(providerId) {
  clearTimeout(saveTimers.get(providerId));
  saveTimers.set(providerId, setTimeout(() => {
    saveTimers.delete(providerId);
    void saveVisibleNow(providerId);
  }, SAVE_DEBOUNCE_MS));
}

/** PATCH /api/providers/:id с ПОЛНЫМ списком visible_models провайдера.
 *  Семантика PATCH — «замена списка целиком» (admin.rs ProviderPatch), поэтому
 *  последний PATCH несёт полное состояние: гонка «клик во время полёта»
 *  разрешается сама — клик обновляет state до отправки; из ответа DTO берутся
 *  только model-поля (models/models_count), локальный visible_models не
 *  перезаписывается (серверная нормализация trim/dedup для id из p.models
 *  тождественна; stale-выбор вне p.models сервер бережно переносит). */
async function saveVisibleNow(providerId) {
  const p = state.config.providers.find(x => x.id === providerId);
  if (!p) return;
  try {
    const dto = await api(`/api/providers/${encodeURIComponent(providerId)}`, {
      method: 'PATCH',
      body: JSON.stringify({ visible_models: p.visible_models }),
    });
    // Из ответа берём ТОЛЬКО model-поля: локальный visible_models — истина
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
  const chosen = visibleSet(p);
  const total = p.models.length;
  const shown = p.models.filter(m => chosen.has(m)).length;
  badge.textContent = shown === total
    ? `${total} ${plural(total, 'модель', 'модели', 'моделей')} в чате`
    : `в чате ${shown} из ${total}`;
  badge.classList.toggle('gw-warn', shown !== total);
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
