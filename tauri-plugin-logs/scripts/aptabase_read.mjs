#!/usr/bin/env node
// aptabase_read.mjs — чтение отчётов об ошибках и аналитики Aptabase из CLI.
//
// ТОЛЬКО ЧТЕНИЕ: все запросы — GET. POST/PUT к ingestion-эндпоинтам
// (/api/v0/events, /api/v0/error) здесь отсутствуют намеренно — тестовые
// события в прод-данные не подмешиваются.
//
// Аутентификация — cookie `auth-session` дашборда (у облачного Aptabase нет
// публичного query-API и read-ключей; ключ A-EU-* служит только для отправки).
// Источники куки (по порядку):
//   1) env APTABASE_AUTH_SESSION
//   2) файл %APPDATA%\aptabase\auth-session.txt (Linux/macOS: ~/.config/aptabase/)
// Значение куки НИКОГДА не печатается (кука = полный доступ к дашборду).
//
// Прочие эндпоинты и параметры сверены с исходниками:
//   - сервер: github.com/aptabase/aptabase (Features/ErrorReporting/ErrorsController.cs, StatsController.cs)
//   - обёртка: github.com/aptabase/aptabase-mcp (src/client.ts, src/period.ts)
//
// Exit-коды: 0 — успех; 1 — ошибка запроса/формата; 2 — кука не найдена;
// 3 — кука истекла/не принята (HTTP 401).

import { readFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const EXIT_OK = 0;
const EXIT_FAIL = 1;
const EXIT_NO_COOKIE = 2;
const EXIT_AUTH_EXPIRED = 3;
const HTTP_TIMEOUT_MS = 20_000;

// Дашборд-пресеты периода (aptabase-mcp src/period.ts, PERIODS).
const PERIODS = [
  "24h", "48h", "today", "yesterday", "7d", "14d", "30d",
  "month", "last-month", "90d", "180d", "365d", "all",
];

const SEVERITIES = ["fatal", "error"];
const KINDS = ["crash", "unhandled", "taskException", "handled"];

// ---------------------------------------------------------------------------
// Вывод и ошибки: каждая неудача печатает статус и причину, без стек-трейса.
// ---------------------------------------------------------------------------

function fail(message, code = EXIT_FAIL) {
  console.error(`[aptabase_read] ${message}`);
  process.exit(code);
}

function trace(line) {
  // След запроса/ответа: в обычном режиме — stdout (виден вместе с таблицей),
  // в --json — stderr, чтобы stdout остался чистым JSON для пайпа.
  if (state.json) console.error(`[aptabase_read] ${line}`);
  else console.log(`-- ${line}`);
}

const state = { json: false };

// ---------------------------------------------------------------------------
// Конфиг и кука
// ---------------------------------------------------------------------------

function configDir() {
  if (process.env.APPDATA) return join(process.env.APPDATA, "aptabase");
  return join(homedir(), ".config", "aptabase");
}

function loadConfig() {
  const path = join(configDir(), "config.json");
  if (!existsSync(path)) return { path, baseUrl: null, appId: null };
  try {
    const raw = JSON.parse(readFileSync(path, "utf8"));
    return { path, baseUrl: raw.baseUrl ?? null, appId: raw.appId ?? null };
  } catch (e) {
    fail(`конфиг ${path} не читается как JSON: ${e.message}. Исправь файл или удали его.`);
  }
}

function cookieValue() {
  const env = process.env.APTABASE_AUTH_SESSION;
  if (env && env.trim()) return normalizeAuthSession(env);
  const path = join(configDir(), "auth-session.txt");
  if (!existsSync(path)) {
    fail(
      `кука auth-session не найдена.\n` +
      `Как получить: открой https://eu.aptabase.com в браузере и войди →\n` +
      `  DevTools (F12) → Application → Cookies → https://eu.aptabase.com →\n` +
      `  скопируй значение куки auth-session и сохрани в файл:\n` +
      `  ${path}\n` +
      `(или передай через env APTABASE_AUTH_SESSION). Срок жизни куки ~365 дней;\n` +
      `при 401 — обнови файл. Подробнее: docs/README.md, раздел про aptabase_read.`,
      EXIT_NO_COOKIE,
    );
  }
  return normalizeAuthSession(readFileSync(path, "utf8"));
}

// Терпим вставку как значения, так и всей строки из DevTools:
// "auth-session=..." или "Cookie: auth-session=...; other=...".
function normalizeAuthSession(raw) {
  const m = raw.match(/auth-session=([^;\s]+)/);
  return m ? m[1] : raw.trim();
}

// ---------------------------------------------------------------------------
// Даты (Mapping пресетов — aptabase-mcp src/period.ts, resolvePeriod)
// ---------------------------------------------------------------------------

function iso(d) { return d.toISOString(); }
function endOfMinute(d) { const x = new Date(d); x.setSeconds(59, 999); return x; }
function endOfDay(d) { const x = new Date(d); x.setHours(23, 59, 59, 999); return x; }

function resolvePeriod(period, now = new Date()) {
  const end = endOfMinute(now);
  switch (period) {
    case "24h": return { startDate: iso(new Date(end.getTime() - 24 * 3600e3)), endDate: iso(end), granularity: "hour" };
    case "48h": return { startDate: iso(new Date(end.getTime() - 48 * 3600e3)), endDate: iso(end), granularity: "hour" };
    case "today": return { startDate: iso(new Date(now.getFullYear(), now.getMonth(), now.getDate())), endDate: iso(end), granularity: "hour" };
    case "yesterday": {
      const s = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
      return { startDate: iso(s), endDate: iso(endOfDay(s)), granularity: "hour" };
    }
    case "month": return { startDate: iso(new Date(now.getFullYear(), now.getMonth(), 1)), endDate: iso(endOfDay(now)), granularity: "day" };
    case "last-month": {
      const s = new Date(now.getFullYear(), now.getMonth() - 1, 1);
      const e = new Date(now.getFullYear(), now.getMonth(), 0, 23, 59, 59, 999);
      return { startDate: iso(s), endDate: iso(e), granularity: "day" };
    }
    case "all": return { startDate: iso(new Date(0)), endDate: iso(endOfDay(now)), granularity: "month" };
    default: {
      const days = { "7d": 7, "14d": 14, "30d": 30, "90d": 90, "180d": 180, "365d": 365 }[period];
      if (!days) fail(`неизвестный период "${period}". Допустимо: ${PERIODS.join(", ")}`);
      const gran = days >= 180 ? "month" : "day";
      return { startDate: iso(new Date(endOfDay(now).getTime() - days * 864e5)), endDate: iso(endOfDay(now)), granularity: gran };
    }
  }
}

// Диапазон для errors: endDate — конец сегодня, startDate — на N дней назад.
function rangeFromDays(days, now = new Date()) {
  const end = endOfDay(now);
  return { startDate: iso(new Date(end.getTime() - days * 864e5)), endDate: iso(end) };
}

// ---------------------------------------------------------------------------
// HTTP (только GET)
// ---------------------------------------------------------------------------

async function apiGet(baseUrl, path, query = {}) {
  const url = new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`);
  for (const [k, v] of Object.entries(query)) {
    if (v !== undefined && v !== null && v !== "") url.searchParams.set(k, String(v));
  }
  const cookie = cookieValue(); // отказ «нет куки» — до печати следа запроса
  trace(`GET ${url.pathname}${url.search}`);
  let resp;
  try {
    resp = await fetch(url, {
      headers: { Accept: "application/json", Cookie: `auth-session=${cookie}` },
      signal: AbortSignal.timeout(HTTP_TIMEOUT_MS),
    });
  } catch (e) {
    const reason = e?.name === "TimeoutError" ? `таймаут ${HTTP_TIMEOUT_MS / 1000} с` : e?.cause?.code ?? e?.message ?? "неизвестная сетевая ошибка";
    fail(`сеть: запрос к ${url.host} не выполнен (${reason}). Проверь сеть/VPN и доступность ${baseUrl}.`);
  }

  if (resp.status === 401) {
    fail(
      `кука истекла/не принята (HTTP 401). Обнови auth-session:\n` +
      `  войди на ${baseUrl} в браузере → DevTools → Application → Cookies →\n` +
      `  скопируй auth-session в ${join(configDir(), "auth-session.txt")}`,
      EXIT_AUTH_EXPIRED,
    );
  }
  if (resp.status === 403) fail(`доступ запрещён (HTTP 403): у этой учётки нет доступа к приложению (или appId чужой).`);
  if (resp.status === 404) fail(`не найдено (HTTP 404): проверь appId — прогони list-apps и пропиши id в ${join(configDir(), "config.json")}.`);
  if (resp.status === 429) fail(`rate limit (HTTP 429): лимит дашборда ~1000 запросов/час — подожди и повтори позже.`);

  const text = await resp.text();
  if (resp.status !== 200) {
    fail(`HTTP ${resp.status} от ${url.pathname}. Тело (первые 300 символов): ${text.slice(0, 300)}`);
  }
  if (/^\s*</.test(text)) {
    fail(`сервер вернул страницу (HTML), не JSON. Первые строки:\n${text.split("\n").slice(0, 5).join("\n").slice(0, 500)}`);
  }
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    fail(`ответ не разбирается как JSON (content-type: ${resp.headers.get("content-type") ?? "?"}). Начало: ${text.slice(0, 300)}`);
  }
  return data;
}

// ---------------------------------------------------------------------------
// Рендеры
// ---------------------------------------------------------------------------

function appList(data) {
  const apps = Array.isArray(data) ? data : Array.isArray(data?.apps) ? data.apps : null;
  if (!apps) fail(`неожиданный формат ответа /api/_apps: ожидался массив приложений, пришло: ${JSON.stringify(data).slice(0, 300)}`);
  for (const a of apps) {
    console.log(`${a.id ?? "?"} | ${a.name ?? "?"} | ${a.appKey ?? "?"}`);
  }
  console.log(`\n-- всего приложений: ${apps.length}`);
  console.log(JSON.stringify(data, null, 2));
}

function errorRow(e) {
  const msg = String(e.errorMessage ?? "").replace(/\s+/g, " ").slice(0, 120);
  const cells = [
    e.timestamp ?? "?",
    e.errorType ?? "?",
    msg,
    e.osName ?? "",
    e.appVersion ?? "",
    e.severity ?? "",
    e.kind ?? "",
    e.errorId ?? "",
  ];
  console.log(cells.join(" | "));
}

function printErrors(data) {
  const errors = data?.errors;
  if (!Array.isArray(errors)) {
    fail(`неожиданный формат ответа: ожидался объект {errors:[…], pagination:{…}}, пришло: ${JSON.stringify(data).slice(0, 300)}`);
  }
  for (const e of errors) errorRow(e);
  const p = data.pagination ?? {};
  trace(`получено ${errors.length} записей, всего за период: ${p.total ?? "?"} (offset ${p.offset ?? "?"}, limit ${p.limit ?? "?"})`);
}

function printStats(data, countHint) {
  // Формы /api/_stats/* у сервера разнятся между эндпоинтами — печатаем
  // честный JSON целиком (маленький), а след считаем по массивам, если есть.
  console.log(JSON.stringify(data, null, 2));
  const arr = Array.isArray(data) ? data : Array.isArray(data?.rows) ? data.rows : null;
  trace(countHint ?? (arr ? `получено строк: ${arr.length}` : `получен объект ${Object.keys(data ?? {}).join(", ") || "(пустой)"}`));
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const FLAGS_WITH_VALUE = new Set([
  "--days", "--from", "--to", "--type", "--severity", "--kind", "--os",
  "--limit", "--offset", "--app", "--base-url", "--period", "--event",
]);

function parseArgs(argv) {
  const opts = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--json") { opts.json = true; continue; }
    if (a === "--debug") { opts.debug = true; continue; }
    if (FLAGS_WITH_VALUE.has(a)) {
      const v = argv[++i];
      if (v === undefined) fail(`флаг ${a} требует значения`);
      opts[a.slice(2)] = v;
      continue;
    }
    if (a.startsWith("--")) fail(`неизвестный аргумент "${a}". Справка: node scripts/aptabase_read.mjs help`);
    opts._.push(a); // позиционные (например, error-detail <errorId>)
  }
  return opts;
}

function requireInt(name, v, { min = 1, max = Infinity } = {}) {
  const n = Number(v);
  if (!Number.isInteger(n) || n < min || n > max) fail(`--${name} ожидает целое от ${min} до ${max === Infinity ? "∞" : max}, получено: "${v}"`);
  return n;
}

function validateEnum(name, v, allowed) {
  if (!allowed.includes(v)) fail(`--${name} ожидает одно из: ${allowed.join("|")}, получено: "${v}"`);
  return v;
}

function dateRange(opts) {
  if (opts.days !== undefined) return rangeFromDays(requireInt("days", opts.days));
  if (opts.from !== undefined || opts.to !== undefined) {
    if (opts.from === undefined || opts.to === undefined) fail(`--from и --to задаются парой`);
    for (const d of [opts.from, opts.to]) if (!/^\d{4}-\d{2}-\d{2}$/.test(d)) fail(`--from/--to ожидает YYYY-MM-DD, получено: "${d}"`);
    return { startDate: iso(new Date(`${opts.from}T00:00:00`)), endDate: iso(new Date(`${opts.to}T23:59:59.999`)) };
  }
  return rangeFromDays(7);
}

function context(cfg, opts) {
  const baseUrl = opts["base-url"] ?? cfg.baseUrl ?? "https://eu.aptabase.com";
  const appId = opts.app ?? cfg.appId;
  return { baseUrl, appId, buildMode: opts.debug ? "debug" : "release" };
}

function requireAppId(ctx) {
  if (!ctx.appId) {
    fail(`appId не задан. Прогони "list-apps", найди нужное приложение и пропиши его id в ${join(configDir(), "config.json")} ("appId": "…"), либо передай --app <id>.`);
  }
  return ctx.appId;
}

const HELP = `aptabase_read.mjs — чтение отчётов об ошибках и аналитики Aptabase (только GET).

Использование: node scripts/aptabase_read.mjs <команда> [опции]

Команды:
  list-apps                          приложения учётки (GET /api/_apps)
  errors                             список ошибок (GET /api/v0/apps/{appId}/errors)
  error-types                        уникальные типы ошибок (…/errors/types)
  error-detail <errorId>             деталь одной ошибки (полный стек)
  metrics [--period P]               DAU/сессии/события (GET /api/_stats/metrics)
  top-events [--period P] [--event N] топ событий (+ разбивка по свойствам)
  top-versions [--period P]          топ версий приложения
  usage                              месячный расход событий (GET /api/_export/usage)

Опции:
  --days N                           период ошибок: последние N дней (по умолчанию 7)
  --from YYYY-MM-DD --to YYYY-MM-DD  период ошибок явно (вместо --days)
  --type T --severity fatal|error    фильтры ошибок
  --kind crash|unhandled|taskException|handled
  --os NAME                          фильтр по ОС
  --limit N (1..100) --offset N      постраничность (по умолчанию limit 50)
  --period P                         пресет статистик: ${PERIODS.join(", ")}
  --event NAME                       имя события для top-props
  --app <appId>                      переопределить appId из конфига
  --debug                            buildMode=debug (дев-запуски); по умолчанию release
  --base-url <url>                   переопределить baseUrl (по умолчанию https://eu.aptabase.com)
  --json                             чистый JSON в stdout (след запроса уходит в stderr)

Кука: env APTABASE_AUTH_SESSION или файл ${join(configDir(), "auth-session.txt")}.
Конфиг: ${join(configDir(), "config.json")} → {"baseUrl": "…", "appId": "…"}.

Примеры:
  node scripts/aptabase_read.mjs list-apps
  node scripts/aptabase_read.mjs errors --days 30 --kind crash
  node scripts/aptabase_read.mjs error-detail 1a2b3c-…
  node scripts/aptabase_read.mjs metrics --period 30d
`;

async function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  if (!cmd || cmd === "help" || cmd === "--help" || cmd === "-h") {
    console.log(HELP);
    process.exit(EXIT_OK);
  }
  const opts = parseArgs(rest);
  state.json = opts.json === true;
  const cfg = loadConfig();
  const ctx = context(cfg, opts);
  const range = () => (opts.period ? { ...resolvePeriod(opts.period) } : null);

  switch (cmd) {
    case "list-apps": {
      const data = await apiGet(ctx.baseUrl, "/api/_apps");
      appList(data);
      break;
    }
    case "errors": {
      const appId = requireAppId(ctx);
      if (opts.severity !== undefined) validateEnum("severity", opts.severity, SEVERITIES);
      if (opts.kind !== undefined) validateEnum("kind", opts.kind, KINDS);
      const { startDate, endDate } = dateRange(opts);
      const limit = opts.limit !== undefined ? requireInt("limit", opts.limit, { min: 1, max: 100 }) : 50;
      const offset = opts.offset !== undefined ? requireInt("offset", opts.offset, { min: 0 }) : 0;
      const data = await apiGet(ctx.baseUrl, `/api/v0/apps/${appId}/errors`, {
        buildMode: ctx.buildMode, startDate, endDate,
        errorType: opts.type, osName: opts.os, severity: opts.severity, kind: opts.kind,
        offset, limit,
      });
      if (opts.json) console.log(JSON.stringify(data, null, 2));
      else printErrors(data);
      break;
    }
    case "error-types": {
      const appId = requireAppId(ctx);
      const { startDate, endDate } = dateRange(opts);
      const data = await apiGet(ctx.baseUrl, `/api/v0/apps/${appId}/errors/types`, {
        buildMode: ctx.buildMode, startDate, endDate,
      });
      if (opts.json) { console.log(JSON.stringify(data, null, 2)); break; }
      const rows = Array.isArray(data) ? data : data?.types ?? null;
      if (rows === null) fail(`неожиданный формат ответа types: ${JSON.stringify(data).slice(0, 300)}`);
      for (const r of rows) {
        const t = typeof r === "string" ? r : r.errorType ?? r.type ?? JSON.stringify(r);
        console.log(`${t} | ${r.count ?? ""}`);
      }
      trace(`получено типов: ${rows.length}`);
      break;
    }
    case "error-detail": {
      const appId = requireAppId(ctx);
      const errorId = opts._[0];
      if (!errorId) fail(`использование: error-detail <errorId> (id берётся из вывода команды errors)`);
      const data = await apiGet(ctx.baseUrl, `/api/v0/apps/${appId}/errors/${encodeURIComponent(errorId)}`, {
        buildMode: ctx.buildMode,
      });
      console.log(JSON.stringify(data, null, 2));
      trace(`деталь ошибки ${errorId} получена`);
      break;
    }
    case "metrics":
    case "top-versions": {
      const appId = requireAppId(ctx);
      const p = range() ?? resolvePeriod("24h");
      const path = cmd === "metrics" ? "/api/_stats/metrics" : "/api/_stats/top-appversions";
      const data = await apiGet(ctx.baseUrl, path, {
        appId, buildMode: ctx.buildMode,
        startDate: p.startDate, endDate: p.endDate, granularity: p.granularity,
      });
      if (opts.json) { console.log(JSON.stringify(data, null, 2)); break; }
      printStats(data);
      break;
    }
    case "top-events": {
      const appId = requireAppId(ctx);
      const p = range() ?? resolvePeriod("24h");
      const base = { appId, buildMode: ctx.buildMode, startDate: p.startDate, endDate: p.endDate, granularity: p.granularity };
      const data = await apiGet(ctx.baseUrl, "/api/_stats/top-events", { ...base, eventName: opts.event });
      if (opts.json) console.log(JSON.stringify(data, null, 2));
      else printStats(data);
      if (opts.event) {
        const props = await apiGet(ctx.baseUrl, "/api/_stats/top-props", { ...base, eventName: opts.event });
        console.log("\n## Разбивка по свойствам события:\n");
        if (opts.json) console.log(JSON.stringify(props, null, 2));
        else printStats(props);
      }
      break;
    }
    case "usage": {
      const appId = requireAppId(ctx);
      const data = await apiGet(ctx.baseUrl, "/api/_export/usage", { appId, buildMode: ctx.buildMode });
      if (opts.json) { console.log(JSON.stringify(data, null, 2)); break; }
      printStats(data, "месячный расход событий получен (квота free-тарифа — 20 000/мес)");
      break;
    }
    default:
      fail(`неизвестная команда "${cmd}". Справка: node scripts/aptabase_read.mjs help`);
  }
  process.exit(EXIT_OK);
}

main();
