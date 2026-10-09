// Vercel Edge Function — релей для cloud-routers-gateway.
//
// Зачем: провайдеры бывают недоступны из региона пользователя. Эта функция
// даёт выход с чужой территории, шлюз шлёт через неё запрос.
//
// ЗАЩИТА. Воркер развёрнут публично. Без проверки секрета любой, кто узнает URL
// (логи, скриншот, чужая ссылка), получил бы бесплатный прокси «куда угодно»
// и расходовал бы квоту владельца деплоя. Поэтому:
//   1. x-relay-token обязателен и сверяется с env-переменной RELAY_TOKEN;
//   2. ALLOWED_HOSTS (необязательно) ограничивает список целевых хостов.
//
// ТОЧКА ОСТОРОЖНОСТИ: админ может выставить RELAY_TOKEN и ALLOWED_HOSTS через
// переменные окружения деплоя. Если RELAY_TOKEN не задан — воркер отвечает 503
// и НИЧЕГО не проксирует (fail-closed, а не fail-open).
//
// Абсолютный URL цели приходит в x-target-url. Схема «x-target-host + pathname»
// даёт двойной /v1 у провайдеров, чей base_url уже заканчивается на /v1.

export const config = { runtime: 'edge' }

/** Проверить секрет запроса против переменной окружения. */
function authorize(req) {
  const expected = process.env.RELAY_TOKEN
  if (!expected) {
    return { ok: false, status: 503, message: 'relay is not configured: RELAY_TOKEN env var is missing' }
  }
  const provided = req.headers.get('x-relay-token')
  if (!provided || provided !== expected) {
    return { ok: false, status: 401, message: 'missing or invalid x-relay-token' }
  }
  return { ok: true }
}

/** Проверить, что цель не выходит за ALLOWED_HOSTS. */
function targetAllowed(url) {
  const allow = process.env.ALLOWED_HOSTS
  if (!allow) return true
  const allowed = allow.split(',').map((s) => s.trim().toLowerCase()).filter(Boolean)
  if (allowed.length === 0) return true
  const host = url.hostname.toLowerCase()
  return allowed.some((entry) => (entry.startsWith('*.') ? host.endsWith(entry.slice(1)) : host === entry))
}

/**
 * Обработчик запроса.
 *
 * Поток ответа возвращается как есть — без буферизации, поэтому SSE-поток
 * инференса доходит до IDE с той же задержкой, что и при прямом подключении.
 */
export default async function handler(req) {
  const auth = authorize(req)
  if (!auth.ok) {
    return new Response(JSON.stringify({ error: { message: auth.message, type: 'authentication_error' } }), {
      status: auth.status,
      headers: { 'content-type': 'application/json' },
    })
  }

  const rawTarget = req.headers.get('x-target-url')
  if (!rawTarget) {
    // Этот же ответ использует шлюз как признак «живого воркера» —
    // см. probe_relay в Rust.
    return new Response(JSON.stringify({ error: { message: 'missing x-target-url header', type: 'invalid_request_error' } }), {
      status: 400,
      headers: { 'content-type': 'application/json' },
    })
  }

  let target
  try {
    target = new URL(rawTarget)
  } catch {
    return new Response(JSON.stringify({ error: { message: 'malformed x-target-url', type: 'invalid_request_error' } }), {
      status: 400,
      headers: { 'content-type': 'application/json' },
    })
  }

  if (target.protocol !== 'https:' && target.protocol !== 'http:') {
    return new Response(JSON.stringify({ error: { message: 'only http(s) targets are allowed', type: 'invalid_request_error' } }), {
      status: 400,
      headers: { 'content-type': 'application/json' },
    })
  }

  if (!targetAllowed(target)) {
    return new Response(JSON.stringify({ error: { message: 'target host is not in ALLOWED_HOSTS', type: 'permission_error' } }), {
      status: 403,
      headers: { 'content-type': 'application/json' },
    })
  }

  const headers = new Headers(req.headers)
  // Служебные заголовки нашего протокола и запрещённые для пересылки.
  for (const h of ['x-target-url', 'x-relay-token', 'host', 'content-length', 'connection', 'accept-encoding']) {
    headers.delete(h)
  }

  const hasBody = req.method !== 'GET' && req.method !== 'HEAD'
  try {
    return await fetch(target.toString(), {
      method: req.method,
      headers,
      body: hasBody ? req.body : undefined,
      redirect: 'manual',
    })
  } catch (e) {
    return new Response(JSON.stringify({ error: { message: `upstream fetch failed: ${e.message || e}`, type: 'api_connection_error' } }), {
      status: 502,
      headers: { 'content-type': 'application/json' },
    })
  }
}