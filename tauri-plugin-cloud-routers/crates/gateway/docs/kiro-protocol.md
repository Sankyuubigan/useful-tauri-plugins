# Kiro: протокол и статус доказательств

Kiro — не публичный API, а реверс закрытого IDE-протокола Amazon Q (9Router).
Каждый пункт ниже имеет статус: что подтверждено кодом 9Router (raw-файлы,
прочитаны 2026-10-17), а что — UNVERIFIED до живого зонда.

## OAuth (Device Code Flow)

| Пункт | Значение | Статус |
|---|---|---|
| Регистрация клиента | `POST https://oidc.{region}.amazonaws.com/client/register`, тело `{clientName, clientType, scopes, grantTypes, issuerUrl}` | Подтверждено: `src/lib/oauth/providers/kiro.js` (`registerClientUrl`, `requestDeviceCode`) |
| Начало device flow | `POST https://oidc.{region}.amazonaws.com/device_authorization` `{clientId, clientSecret, startUrl}` → `deviceCode/userCode/verificationUri*` | Подтверждено: тот же файл |
| Поллинг токена | `POST .../token` `{clientId, clientSecret, deviceCode, grantType: urn:ietf:params:oauth:grant-type:device_code}` | Подтверждено: `pollToken` в том же файле |
| Refresh Builder ID/IDC | `POST .../token` `{clientId, clientSecret, refreshToken, grantType: refresh_token}` | Подтверждено: `src/lib/oauth/services/kiro.js` (`refreshToken`, ветка `clientId && clientSecret`) |
| Соцвход Google/GitHub | `GET {KIRO_AUTH_SERVICE}/login?idp=…&redirect_uri=kiro://…&code_challenge=…` → обмен `POST {KIRO_AUTH_SERVICE}/oauth/token` → refresh `POST {KIRO_AUTH_SERVICE}/refreshToken` | Подтверждено: тот же файл (`buildSocialLoginUrl`, `exchangeSocialCode`) |
| `KIRO_AUTH_SERVICE` | `https://prod.us-east-1.auth.desktop.kiro.dev` | Подтверждено: константа в том же файле |
| Region allowlist | `^[a-z]{2}-[a-z]+-\d{1,2}$`, иначе «Invalid region» (защита от SSRF через подстановку региона в URL) | Подтверждено: `src/lib/oauth/constants/oauth.js` (`AWS_REGION_PATTERN`, `assertValidAwsRegion`) |

## Каталог моделей

| Пункт | Значение | Статус |
|---|---|---|
| OAuth/IDC-профили | `POST https://codewhisperer.{region}.amazonaws.com`, заголовки `Content-Type: application/x-amz-json-1.0`, `x-amz-target: AmazonCodeWhispererService.ListAvailableProfiles`, тело `{maxResults}` | Подтверждено: `listAvailableProfiles` в `services/kiro.js` |
| API-ключ → модели | `GET https://q.{region}.amazonaws.com/ListAvailableModels?origin=AI_EDITOR`, заголовки `Authorization: Bearer`, `TokenType: API_KEY` | Подтверждено: `listAvailableApiKeyModels` в том же файле |
| Пустой список ≠ валидный ключ | 9Router считает пустой `models[]` провалом валидации ключа | Подтверждено: `throw` в той же функции |
| `profileArn` в каталоге | query-параметр `profileArn` для OAuth; API-ключ — без профиля | Подтверждено: `open-sse/services/kiroModels.js` |

## Инференс

| Пункт | Значение | Статус |
|---|---|---|
| Endpoints, порядок | `runtime.us-east-1.kiro.dev/generateAssistantResponse` → `codewhisperer.us-east-1.amazonaws.com/generateAssistantResponse` → `q.us-east-1.amazonaws.com/generateAssistantResponse`; 401/403/404 = пробовать следующий | Подтверждено: `open-sse/providers/registry/kiro.js` (`transport.baseUrl/baseUrls`), `open-sse/executors/kiro.js` (`getOrderedBaseUrls`), `KIRO_ENDPOINT_FALLBACK_STATUSES` |
| Заголовки | `Content-Type: application/json`, `Accept: application/vnd.amazon.eventstream`, `Authorization: Bearer`, `TokenType: API_KEY` (ключи) / `x-amz-sso-bearer` (OAuth), `x-amzn-kiro-agent-mode: spec`, `x-amzn-codewhisperer-machine-id: kiro-desktop`, опц. `x-amzn-codewhisperer-profile-arn`, `X-Amz-Target` только на `codewhisperer.*` | Подтверждено: `buildHeaders` в `executors/kiro.js` |
| Тело | `{conversationState: {chatTriggerType: MANUAL, conversationId, currentMessage: {userInputMessage: {content, modelId, origin: AI_EDITOR, …}}, history: [...]}, profileArn?, inferenceConfig?}` | Подтверждено: `open-sse/translator/request/openai-to-kiro.js` |
| Ответ | бинарный AWS EventStream; события `assistantResponseEvent/toolUseEvent/messageStopEvent/metadataEvent/meteringEvent` | Подтверждено: `open-sse/utils/awsEventStream.js` + `transformEventStreamToSSE` |
| Декодер инвертирован | MITM-хендлер 9Router (`src/mitm/handlers/kiro.js`) делает Kiro→OpenAI, нам нужно OpenAI→Kiro; переиспользуем только формат фреймов, не направление | Подтверждено чтением обоих файлов |

## UNVERIFIED (только живой зонд с логином пользователя)
- Реальные shape/коды ответов AWS (`401` vs `403` на мёртвом ключе, формат `models[]`).
- Срок жизни access-токена Builder ID (в коде 9Router `expiresIn || 3600` — значение по умолчанию, не факт).
- Не сносит ли повторный вход другие сессии (по дизайну OAuth — нет: каждая device-сессия своя; проверить зондом).

## Правило двух мест
Свой device flow шлюза — свои токены. Refresh-токен из 9Router не переиспользуем:
два потребителя одного refresh-токена устраивают гонку, второй рефреш со старым
токеном падает. Это не предположение, а следствие одноразовости refresh-ротации.
