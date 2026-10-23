// Общие утилиты дашборда шлюза (ES-модуль).
//
// Разделение файлов — «по домену», а не «по размеру»:
//   utils.js     — DOM-хелперы, fetch, тосты (импортируют все);
//   providers.js — карточки провайдеров, ключи, OAuth, модалки;
//   app.js       — вкладки, модели, релей, журнал, каталог пресетов, init.
//
// Файлы ничего не запоминают между перезагрузками: единственный источник
// правды — REST API шлюза (см. src/admin.rs). После изменения конфигурации
// хост уведомляется через postMessage (notifyHost).

export const $ = (sel) => document.querySelector(sel);
export const $$ = (sel) => Array.from(document.querySelectorAll(sel));

export const state = {
  config: null,
  status: null,
  presets: null,
  events: [],
  lastSeq: 0,
  logTimer: null,
};

export async function api(path, options = {}) {
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

export function toast(message, kind = 'ok') {
  const el = $('#toast');
  el.textContent = message;
  el.className = `gw-toast ${kind === 'err' ? 'gw-err' : 'gw-ok'}`;
  el.hidden = false;
  clearTimeout(el._t);
  el._t = setTimeout(() => { el.hidden = true; }, 4200);
}

export function fail(where, e) {
  console.error(where, e);
  toast(`${where}: ${e.message || e}`, 'err');
}

export function busy(selector, on) {
  const el = typeof selector === 'string' ? $(selector) : selector;
  if (!el) return;
  el.disabled = on;
  el.classList.toggle('gw-busy', on);
}

export function esc(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;')
    .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

export function plural(n, one, few, many) {
  const m10 = n % 10, m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 10 || m100 >= 20)) return few;
  return many;
}

/** Сообщить хосту, что список провайдеров изменился. */
export function notifyHost(reason) {
  if (window.parent && window.parent !== window) {
    try {
      window.parent.postMessage(
        { source: 'cloud-routers-gateway', type: 'config-changed', reason },
        '*'
      );
    } catch { /* хост может быть в песочнице — это не повод ломать UI */ }
  }
}

/**
 * Разобрать текстовое поле ключей формата «имя|секрет» (по строке на ключ).
 *
 * Каждая строка: `имя|ключ` или просто `ключ`. Разделитель — ПЕРВЫЙ `|`,
 * по краям обрезаются пробелы. Возврат — массив `{ name, secret }`, где
 * `name` — строка или null. Пустые строки пропускаются; строка вида
 * «имя|» (с именем, но без секрета) возвращается как есть — валидность
 * решает вызывающий, чтобы пользователь увидел ошибку, а не молчаливую
 * потерю ключа.
 */
export function parseKeySpecs(text) {
  return String(text || '')
    .split('\n')
    .map(line => line.trim())
    .filter(Boolean)
    .map((line) => {
      const cut = line.indexOf('|');
      if (cut === -1) {
        return { name: null, secret: line.trim() };
      }
      let name = line.slice(0, cut).trim();
      const secret = line.slice(cut + 1).trim();
      return { name: name || null, secret };
    });
}

/** Непустые строки с именем, но без секрета — читаемый список для ошибки. */
export function keySpecsWithNameButNoSecret(parsed) {
  return parsed.filter(x => x.name && !x.secret).map(x => x.name);
}

/**
 * Элементы для POST: именованный → `{ name, secret }`, безымянный → строка.
 * Секрет обязателен: записи без секрета дросселируются НЕ здесь, а до вызова.
 */
export function keySpecsForPayload(parsed) {
  return parsed
    .filter(x => x.secret)
    .map(x => (x.name === null ? x.secret : { name: x.name, secret: x.secret }));
}
