# Обзор API

CrabIndex отдаёт все API на одном порту (по умолчанию `9117`, ключ `listenport`) и работает с одной базой FileDB. Поисковые API различаются только форматом ответа: Jackett JSON, Torznab XML, лента Prowlarr и собственный JSON CrabIndex.

## Базовый URL

```text
http://<host>:9117
```

## Интерактивная документация

| Адрес | Что это |
| --- | --- |
| `GET /swagger` | Swagger UI: можно отправлять запросы прямо из браузера |
| `GET /openapi.yaml` | Спецификация OpenAPI 3 в YAML |
| `GET /swagger/v1/swagger.json` | Та же спецификация, преобразованная в JSON (для генераторов клиентов) |

Спецификация берётся из `wwwroot/openapi.yaml`, а в дереве исходников из `web/public/openapi.yaml`. Все три адреса открыты без ключей.

```bash
curl http://127.0.0.1:9117/openapi.yaml
curl http://127.0.0.1:9117/swagger/v1/swagger.json
```

:::note[Примечание]
Swagger UI загружает скрипты и стили `swagger-ui-dist` с `cdn.jsdelivr.net`, поэтому браузеру нужен доступ в интернет.
:::

## Поисковые API

| API | Эндпоинт | Клиенты |
| --- | --- | --- |
| [Jackett JSON](jackett.md) | `GET /api/v2.0/indexers/{indexer}/results` | Prisma, Lampa, Stremio-аддоны и другие Jackett-клиенты |
| [Torznab XML](torznab.md) | `GET /torznab/api` | Sonarr, Radarr, Prowlarr (Generic Torznab) |
| [Prowlarr](prowlarr.md) | `GET /api/v1/search`, `/api/v1/indexer/...` | Prowlarr и совместимые клиенты |
| [Нативный API](native.md) | `GET /api/v1.0/torrents` | Собственные интеграции, данные `ffprobe` и `languages` |

## Служебные API

| Группа | Эндпоинты | Доступ |
| --- | --- | --- |
| [Идентификация](conf.md) | `/api/v1.0/conf` | публичный |
| [Health](health.md) | `/health`, `/health/background-jobs`, `/version`, `/lastupdatedb` | публичный |
| [Статистика](stats.md) | `/stats/torrents`, `/stats/tracks`, `/stats/meta` | apikey (если задан) + `openstats` |
| [Синхронизация](sync.md) | `/sync/conf`, `/sync/fdb`, `/sync/fdb/torrents` | публичный + `opensync` |
| [Конфигурация](config.md) | `{admin.path}/api/config`, `{admin.path}/api/config/*` | только сессия админ-панели |
| [Cron](cron.md) | `/cron/...`, `/jsondb/save` | LAN или devkey (или через API админ-панели) |
| [Dev](dev.md) | `/dev/*` | LAN или devkey (или через API админ-панели) |

## API админ-панели

Под `{admin.path}/api/` (по умолчанию `/admin/api/`) работает API [админ-панели](../admin.md): вход и выход, сводка `overview`, логи `logs`, а также Config API, `cron/*`, `dev/*`, `jsondb*`, `stats/*` и `health/background-jobs` через сессию панели. Нужны cookie входа по адресу с токеном, сессия после входа по `devkey` и заголовок `X-Crab-Admin: 1` для изменяющих запросов. Полный список маршрутов - на странице [Админ-панель](../admin.md#api-панели).

## Аутентификация

В CrabIndex два независимых ключа. Оба задаются в `init.yaml`. Пустое значение значит, что ключ не задан (`devkey` при первом запуске генерируется автоматически).

| Ключ | Как передать | Что защищает |
| --- | --- | --- |
| `apikey` | `?apikey=...`, заголовок `X-Api-Key: ...` или `Authorization: Bearer ...` | Поиск (`/api/v2.0/*`, `/api/v1/*`, `/api/v1.0/torrents`, `/torznab/api` и т. д.) и `/stats/*`. Пока `apikey` не задан, эти маршруты открыты |
| `devkey` | заголовок `X-Dev-Key: ...` или `?devkey=...` | `/cron/*`, `/dev/*`, `/jsondb/*`. Если `devkey` не задан, эти маршруты доступны только из LAN или с localhost. Он же - пароль админ-панели |

`apikey` из query-строки проверяется раньше заголовков. Для `devkey` порядок обратный: сначала `X-Dev-Key`, затем `?devkey=`.

Запрос через обратный прокси не считается запросом из LAN, даже если прокси работает на том же хосте. Прокси распознаётся по заголовкам `X-Forwarded-For`, `X-Real-IP`, `Forwarded`, `CF-Connecting-IP` и т. п. Административным маршрутам за прокси всегда нужен `devkey`.

```bash
# Поиск с apikey
curl "http://127.0.0.1:9117/api/v1.0/torrents?search=matrix&apikey=YOUR_API_KEY"
curl -H "X-Api-Key: YOUR_API_KEY" "http://127.0.0.1:9117/api/v1.0/torrents?search=matrix"

# Административный вызов извне
curl -H "X-Dev-Key: YOUR_DEV_KEY" "https://crabindex.example.com/cron/rutor/parse"
```

Подробнее в разделах [Аутентификация](../authentication.md) и [Матрица доступа](../operations/access-matrix.md).

## Регистр путей и параметров

Регистр в путях не важен. Перед маршрутизацией CrabIndex переводит путь и имена query-параметров в нижний регистр и отбрасывает завершающий `/`. Значения параметров не меняются. Эти запросы эквивалентны:

```text
/cron/rutor/ParseAllTask
/CRON/Rutor/parsealltask
/api/v1.0/torrents?Search=Matrix&Exact=true
/api/v1.0/torrents?search=Matrix&exact=true
```

В документации пути записаны так же, как в `Data/crontab` (например, `/cron/rutracker/UpdateTasksParse`).

:::note[Примечание]
Ключ доступа проверяется до нормализации, по исходной строке запроса. Поэтому имена `apikey` и `devkey` в query пишите строчными буквами.
:::

## Коды ответов

| Код | Когда |
| --- | --- |
| `200` | Успех. Многие cron-маршруты отвечают текстом (`ok`, `work`, `disabled`), а не JSON |
| `204` | Ответ на CORS preflight (`OPTIONS`) к защищённому маршруту без ключа |
| `401` | Ключ задан в конфиге, а в запросе его нет или он неверный |
| `403` | Административный маршрут вызван не из LAN, а `devkey` не задан |
| `404` | Маршрут не найден или отключён в конфиге (например, Torznab при `torznab.enable: false`). Так же отвечают `/api/v1.0/config*` и адрес админ-панели без токена входа |
| `429` | Слишком много неудачных попыток входа в админ-панель |
| `500` | Внутренняя ошибка: `{"error":"internal server error"}` |

## CORS и сжатие

CrabIndex отражает в ответе `Origin`, заголовки и метод запроса и разрешает credentials. Поэтому API можно вызывать из браузерных клиентов вроде Lampa с любого origin. Если клиент поддерживает gzip или brotli, ответы сжимаются. Исключение - API админ-панели: он принимает запросы только с того же сайта (`Sec-Fetch-Site: cross-site` и `same-site` отклоняются с `403`), а cookie панели имеют `SameSite=Strict`.

## Прочие публичные адреса

| Путь | Описание |
| --- | --- |
| `/`, `/stats` | Сайт (при `web: true`), см. [Веб-интерфейс](../concepts/web-ui.md) |
| `/opensearch.xml` | Описание OpenSearch для строки поиска браузера |
| `/docs/` | Эта документация |
