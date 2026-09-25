# Prisma, Lampa, Sonarr, Prowlarr

CrabIndex отдаёт одну и ту же FileDB в нескольких форматах. Выберите API под свой клиент. Базовый адрес: `http://ВАШ_ХОСТ:9117`. Если на сервере задан `apikey`, клиент должен его передавать (`?apikey=`, `X-Api-Key` или `Authorization: Bearer`).

| Клиент | API | Адрес |
| --- | --- | --- |
| **[Prisma](https://t.me/prisma_party)** | Jackett JSON | `http://ВАШ_ХОСТ:9117` (запросы на `/api/v2.0/indexers/all/results`) |
| Lampa | Jackett JSON | `http://ВАШ_ХОСТ:9117` (запросы на `/api/v2.0/indexers/all/results`) |
| Sonarr, Radarr | Torznab | `http://ВАШ_ХОСТ:9117/torznab/api` |
| Prowlarr | Generic Torznab | `http://ВАШ_ХОСТ:9117/api/v1/indexer/1/newznab` |
| AIOStreams, Stremio-аддоны | Jackett JSON | `http://ВАШ_ХОСТ:9117` |
| Свои скрипты | Собственный JSON | `http://ВАШ_ХОСТ:9117/api/v1.0/torrents` |

Проверить, что клиент видит сервер и ключ принят:

```bash
curl -s "http://ВАШ_ХОСТ:9117/api/v1.0/conf?apikey=KEY"
# {"jacred":true,"configured":true,"apikey":true,"version":"…"}
```

## Prisma

[Prisma](https://t.me/prisma_party) - основной клиент, под который развивается CrabIndex. Она ищет раздачи через Jackett API, так же как Lampa.

1. В настройках источника торрентов Prisma выберите **Jackett - CrabIndex**.
2. Адрес сервера: `http://ВАШ_ХОСТ:9117`. Если CrabIndex стоит за обратным прокси - его внешний адрес, например `https://crabindex.example.com`.
3. Ключ: значение `apikey` из `init.yaml`. Если ключ на сервере не задан, оставьте поле пустым.

Из карточки фильма или сериала Prisma передаёт название, оригинальное название и год - это карточный поиск с точным совпадением названий, он даёт самую чистую выдачу. Поиск по IMDb или Кинопоиску (`tt…`, `kp…`) CrabIndex превращает в названия через [Alloha](../configuration/alloha.md).

Проверить с сервера, что карточный запрос находит раздачи:

```bash
curl -sG "http://127.0.0.1:9117/api/v2.0/indexers/all/results" \
  --data-urlencode "title=Матрица" \
  --data-urlencode "title_original=The Matrix" \
  --data-urlencode "year=1999"
```

Для Prisma подходят настройки поиска по умолчанию: категории и сезоны клиент фильтрует сам (`search.skipCatFilter: true`). Подробности о параметрах запроса - [Jackett](../api-reference/jackett.md). Новости и поддержка Prisma - в Telegram-канале [@prisma_party](https://t.me/prisma_party).

## Lampa

В настройках парсера Lampa выберите Jackett и укажите адрес сервера `http://ВАШ_ХОСТ:9117`. Поле ключа должно совпадать с `apikey` из `init.yaml`; если ключ на сервере пуст - оставьте поле пустым.

Из карточки фильма Lampa присылает `title`, `title_original`, `year` и `is_serial` - это карточный поиск с точным совпадением названий. Отдельный запрос `query=tt0133093` без этих полей - поиск по ID через Alloha.

Проверка карточного запроса:

```bash
curl -sG "http://127.0.0.1:9117/api/v2.0/indexers/all/results" \
  --data-urlencode "Query=The Matrix" \
  --data-urlencode "title=Матрица" \
  --data-urlencode "title_original=The Matrix" \
  --data-urlencode "year=1999" \
  --data-urlencode "is_serial=1"
```

Текстовый поиск:

```bash
curl -s "http://127.0.0.1:9117/api/v2.0/indexers/all/results?query=матрица"
```

Настройки по умолчанию (`search.skipCatFilter: true`) рассчитаны на Lampa: категории она фильтрует сама. Подробности - [Jackett](../api-reference/jackett.md).

## Sonarr и Radarr

Добавьте индексатор **Torznab** (Settings → Indexers → Add → Torznab → Custom):

| Поле | Значение |
| --- | --- |
| URL | `http://ВАШ_ХОСТ:9117/torznab/api` (или `http://ВАШ_ХОСТ:9117/api/v2.0/indexers/all/results/torznab/api`) |
| API Path | `/api` (по умолчанию) |
| API Key | значение `apikey` |
| Categories | по вкусу; категории перечислены в `t=caps` |

Torznab должен быть включён:

```yaml
torznab:
  enable: true         # при false /torznab/api отвечает 404
  enrichTitles: true   # озвучки в названиях раздач
```

Для серверной фильтрации по категориям и сезону см. профиль «Sonarr / Radarr» в [Конфигурации поиска](../configuration/search.md). Подробности - [Torznab](../api-reference/torznab.md).

## Prowlarr

CrabIndex выглядит для Prowlarr как один агрегированный индексатор с `id=1`.

| Поле | Значение |
| --- | --- |
| Тип | Generic Torznab |
| URL | `http://ВАШ_ХОСТ:9117/api/v1/indexer/1/newznab` |
| API Key | значение `apikey` |

Также доступны `GET /api/v1/indexer` (список индексаторов) и `GET /api/v1/search` (поиск в формате Prowlarr JSON). Подробности - [Prowlarr](../api-reference/prowlarr.md).

## AIOStreams

AIOStreams сам фильтрует сезон и серию. Чтобы сервер не отбрасывал лишнего:

```yaml
search:
  skipSeasonEpisodeFilter: true   # не фильтровать сезон/серию на сервере
  stripSeasonEpisode: true        # добавлять вариант запроса без S01/S01E01
  skipCatFilter: true
```

См. [Конфигурация поиска](../configuration/search.md).

## Собственный JSON API

Для своих интеграций и данных о дорожках (`ffprobe`, `languages`):

```bash
curl -s "http://127.0.0.1:9117/api/v1.0/torrents?search=матрица&sort=sid"
curl -s "http://127.0.0.1:9117/api/v1.0/torrents?search=tt0133093"
curl -s "http://127.0.0.1:9117/api/v1.0/trackers"
```

Параметры - в [Нативном API](../api-reference/native.md). Интерактивно все эндпоинты можно попробовать в Swagger UI: `http://ВАШ_ХОСТ:9117/swagger`, спецификация - `/openapi.yaml`.

## Доступ из интернета

Если клиенты подключаются не из локальной сети, поставьте CrabIndex за [обратный прокси](../deployment/reverse-proxy.md) с HTTPS и обязательно задайте `apikey`. См. [Аутентификация](../authentication.md).
