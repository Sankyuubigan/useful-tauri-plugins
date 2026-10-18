/** Событие «состояние экрана логина» — SSOT-модуль без циклических импортов.
 *
 * Контракт: `LOGIN_STATE_EVENT` — имя Tauri-события, `onLoginState` — подписка.
 * Имя события и подписка живут здесь, потому что `web-components.ts` вызывает
 * подписку НА УРОВНЕ МОДУЛЯ (тост честный даже при закрытой панели). Через
 * `./index` это невозможно: `index.ts` в конце импортирует `./web-components`,
 * ESM-импорты хойстятся, поэтому тело `web-components` выполняется до того,
 * как топ-левел `const` из `index.ts` успевает инициализироваться — любой
 * top-level доступ к `const` из `index.ts` внутри `web-components` даёт
 * TDZ-ReferenceError и валит весь бандл хоста (кейс 18.10.2026: вечный
 * прогрессбар, `Cannot access 'uo' before initialization`, CDP-лог).
 * Отдельный модуль разрывает цикл конструктивно: TDZ невозможен по архитектуре.
 */

import { listen } from '@tauri-apps/api/event'

/** Tauri-событие итога проверки экрана логина: `{ router, outcome, detail }`.
 * Шлётся только по неуспехам (свой пароль, лимитер 429) — на честный тост. */
export const LOGIN_STATE_EVENT = 'cloud-routers-login-state'

/** Payload события «состояние экрана логина». */
export interface LoginStatePayload {
  router: string
  outcome: 'protected' | 'rate_limited'
  /** Человеко-читаемая причина — тост показывает её как есть. */
  detail: string
}

/** Подписка на итог проверки экрана логина (неуспехи: свой пароль, лимитер).
 * Возвращает функцию отписки. */
export function onLoginState(cb: (p: LoginStatePayload) => void): Promise<() => void> {
  return listen<LoginStatePayload>(LOGIN_STATE_EVENT, (e) => cb(e.payload))
}