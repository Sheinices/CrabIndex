# Матрица доступа

CrabIndex назначает каждому запросу политику доступа по его пути. Правила зашиты в код сервера; при старте сервер сверяет таблицу маршрутов с правилами и пишет предупреждение категории `security`, если что-то не совпало.

## Политики

| Политика | Условие доступа |
| --- | --- |
| **Public** | Без ключа |
| **ApiKeyWhenConfigured** | Без ключа, если `apikey` пуст; иначе - с правильным `apikey` |
| **ConfigApi** | Только через админ-панель. Прямой запрос - всегда `404`, из любой сети и с любым ключом |
| **DevAdmin** | Прямое подключение из локальной сети или правильный `devkey` |

Отдельно от этих политик работает [админ-панель](../admin.md): запросы к `{admin.path}` и `{admin.path}/*` проверяет её шлюз ещё до статики и политик. Нужны cookie входа (адрес с `admin.token`) и сессия (вход по `devkey`). Запросы API панели к Config API, `/cron/*`, `/dev/*`, `/jsondb*` и `/stats/*` пропускаются без проверки `apikey` и `devkey`: их заменяет сессия.

`apikey` передаётся как `?apikey=`, заголовок `X-Api-Key` или `Authorization: Bearer …`. `devkey` - заголовок `X-Dev-Key` или `?devkey=`. Путь и имена параметров сравниваются без учёта регистра.

## Маршруты

| Путь | Политика | Назначение |
| --- | --- | --- |
| `/`, `/stats` | Public | Страницы сайта |
| `{admin.path}`, `{admin.path}/*` (по умолчанию `/admin`) | Шлюз админ-панели | Панель и её API. Без действующего токена - ответ несуществующего маршрута |
| `/health`, `/health/background-jobs`, `/version`, `/lastupdatedb` | Public | Состояние сервера |
| `/api/v1.0/conf` | Public | Идентификация и проверка `apikey` |
| `/openapi.yaml`, `/swagger`, `/swagger/*` | Public | Описание API |
| `/opensearch.xml`, `/manifest.webmanifest`, `/sw.js`, `/assets/*`, `/img/*`, `/fonts/*` | Public | Файлы интерфейса |
| Любой существующий файл в `wwwroot/` (например, `/docs/*`, `/trackers.txt`), кроме `wwwroot/admin/` | Public | Статика отдаётся до проверки доступа (при `web: true`) |
| `/sync/*` | Public | Синхронизация; дополнительно проверяется `opensync` |
| `/api/v1.0/config`, `/api/v1.0/config/*` | ConfigApi | Внутренние маршруты Config API; снаружи доступны как `{admin.path}/api/config/*` |
| `/cron/*` | DevAdmin | Парсеры, прогрев Cloudflare, обслуживание |
| `/dev/*` | DevAdmin | Диагностика и миграции |
| `/jsondb`, `/jsondb/*` | DevAdmin | Сохранение FileDB |
| `/api/v1.0/torrents`, `/api/v1.0/trackers`, `/api/v1.0/qualitys` | ApiKeyWhenConfigured | Собственный API |
| `/api/v2.0/indexers/*` | ApiKeyWhenConfigured | Jackett и Torznab |
| `/torznab/api` | ApiKeyWhenConfigured | Torznab |
| `/api/v1/indexer/*`, `/api/v1/search` | ApiKeyWhenConfigured | Prowlarr |
| `/stats/torrents`, `/stats/tracks`, `/stats/meta` | ApiKeyWhenConfigured | JSON статистики; дополнительно проверяется `openstats` |
| Всё остальное (в том числе прежние `/jobs` и `/settings`) | ApiKeyWhenConfigured | Несуществующий маршрут: `404`, или `401` без ключа, если задан `apikey` |

## Контекст клиента

| Откуда запрос | Public | DevAdmin | ApiKeyWhenConfigured | Админ-панель |
| --- | --- | --- | --- | --- |
| Loopback или прямое подключение из локальной сети, **без** proxy-заголовков | разрешён | разрешён | `apikey`, если задан | токен + `devkey` |
| Через обратный прокси на хосте, в Docker или в LAN (есть proxy-заголовки) | разрешён | нужен `devkey` | `apikey`, если задан | токен + `devkey` |
| Из интернета или через туннель | разрешён | нужен `devkey` | `apikey`, если задан | токен + `devkey` |

Локальная сеть не даёт в админ-панели никаких послаблений: токен и вход по `devkey` нужны всегда.

«Локальная сеть» - адреса `127.0.0.0/8`, `::1`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`, `fe80::/10`. Proxy-заголовки - `X-Forwarded-For`, `X-Forwarded-Host`, `X-Forwarded-Proto`, `X-Real-IP`, `Forwarded`, `CF-Connecting-IP`, `CF-Ray`.

Если `devkey` пуст, DevAdmin доступны **только** из локальной сети, а вход в админ-панель невозможен.

:::warning[Внимание]
Обратный прокси не превращает запрос в доверенный. Прокси в той же Docker-сети или LAN тоже требует `devkey` для административных путей.
:::

## Коды ответа

| Ситуация | Код |
| --- | --- |
| `OPTIONS` к закрытому пути (CORS preflight) | `204` |
| ApiKeyWhenConfigured: ключ не передан или неверен | `401` |
| DevAdmin: `devkey` задан на сервере, но не передан или неверен | `401` |
| DevAdmin: `devkey` на сервере не задан, запрос не из локальной сети | `403` |
| ConfigApi: любой прямой запрос | `404` |
| Админ-панель: нет действующей cookie входа | как у несуществующего маршрута (`404` или `401`) |
| API админ-панели: нет сессии | `401` |
| API админ-панели: изменяющий запрос без `X-Crab-Admin: 1` или запрос с чужого сайта | `403` |
| Админ-панель: 5 неудачных входов с одного IP за 10 минут | `429` |

```bash
curl -H "X-Dev-Key: DEVKEY" "https://crabindex.example.com/cron/maintenance/Status"
```

## Прочие заголовки ответа

- **CORS**: разрешён любой origin с credentials (origin, методы и заголовки отражаются из запроса). Для локальных клиентов и публичных путей добавляется `Access-Control-Allow-Private-Network: true`.
- **Безопасность**: `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`, `X-Frame-Options: SAMEORIGIN`, `Permissions-Policy`, `Content-Security-Policy` (для `/swagger` и `/openapi.yaml` CSP не выставляется, для `/docs/` - ослабленная).
- **Админ-панель** дополнительно отвечает с `Cache-Control: no-store`, `Referrer-Policy: no-referrer` и `X-Robots-Tag: noindex, nofollow`.

Используйте разные значения `apikey` и `devkey` и не храните их в репозиториях. Настройка ключей - [Аутентификация](../authentication.md).
