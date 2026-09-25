# Prowlarr API

CrabIndex отдаёт один агрегированный индексер Prowlarr с `id=1` - «CrabIndex (all trackers)». Он объединяет результаты всех разрешённых трекеров и поддерживает REST-обнаружение, Newznab XML и Search Feed в JSON. Как подключить Prowlarr, описано на странице [Lampa, Sonarr, Prowlarr](../clients/overview.md).

Все маршруты этой страницы работают только при `torznab.enable: true` (по умолчанию включено); иначе они отвечают `404`.

**Доступ:** требуется `apikey`, если он задан в конфигурации (`?apikey=`, `X-Api-Key` или `Authorization: Bearer`). См. [Аутентификация](../authentication.md) и [Матрица доступа](../operations/access-matrix.md).

Пути и имена query-параметров не зависят от регистра (`indexerIds` и `indexerids` - одно и то же).

## Маршруты

| Метод и путь | Назначение |
| --- | --- |
| `GET /api/v1/indexer` | Список индексеров (любой метод) |
| `GET /api/v1/indexer/{id}` | Данные индексера; существует только `id=1` (любой метод) |
| `GET`, `HEAD /api/v1/indexer/{indexer}/newznab` | Torznab/Newznab XML - см. [Torznab API](torznab.md) |
| `GET /api/v1/search` | Search Feed в JSON (только `GET`) |

## GET /api/v1/indexer

```bash
curl "http://localhost:9117/api/v1/indexer?apikey=KEY"
```

```json
[
  {
    "id": 1,
    "name": "CrabIndex (all trackers)",
    "description": "Aggregated CrabIndex search across all configured trackers",
    "implementation": "Torznab",
    "implementationName": "Torznab",
    "enable": true,
    "protocol": "torrent"
  }
]
```

## GET /api/v1/indexer/{id}

Для `id=1` возвращает описание агрегированного индексера. Любой другой или нечисловой `id` - `404`.

```bash
curl "http://localhost:9117/api/v1/indexer/1?apikey=KEY"
```

```json
{
  "id": 1,
  "name": "CrabIndex (all trackers)",
  "description": "Aggregated CrabIndex search across all configured trackers",
  "implementation": "Torznab",
  "implementationName": "Torznab",
  "enable": true,
  "fields": []
}
```

## GET /api/v1/indexer/{indexer}/newznab

Тот же обработчик, что и `/torznab/api`: `t=caps`, `t=indexers`, `t=search`/`tvsearch`/`moviesearch`. В качестве `{indexer}` можно передать `1` или `all` (все трекеры) либо slug трекера. Параметры и формат ответа описаны на странице [Torznab API](torznab.md).

Для подключения в Prowlarr выберите **Generic Torznab** и укажите URL `http://<host>:9117/api/v1/indexer/1/newznab` (или `http://<host>:9117/torznab/api`) и ключ `apikey`.

```bash
curl "http://localhost:9117/api/v1/indexer/1/newznab?t=caps&apikey=KEY"
```

## GET /api/v1/search

Search Feed: поиск с ответом в формате ReleaseResource Prowlarr.

### Параметры

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `query` | string | - | Поисковая строка. Может содержать токены Prowlarr (см. ниже) |
| `q` | string | - | Используется, если `query` пуст |
| `type` | string | `search` | `search`, `tvsearch` (синоним `tv`), `movie` (синоним `moviesearch`), `music`, `book` |
| `indexerIds` | int, список | все | Не задан - поиск идёт. `1` (CrabIndex) или `-2` (все торрент-индексеры) - поиск идёт. Только `-1` (Usenet) или другие id - пустой массив |
| `categories`, `cat`, `category[]` | int, список | - | Категории Newznab. Если `type` не задаёт тип, по ним определяется фильм или сериал |
| `title`, `title_original` | string | - | Поля карточки; имеют приоритет над разобранными из `query` |
| `year` | int | - | Год выпуска (токен `{Year:…}` имеет приоритет) |
| `season` | int | - | Номер сезона (токен `{Season:…}` имеет приоритет) |
| `ep`, `episode` | int | - | Номер серии (токен `{Episode:…}` имеет приоритет) |
| `is_serial` | int | - | Явно задать тип контента, как в [Jackett API](jackett.md) |
| `genres` | string | - | Жанры |
| `tracker`, `tracker[]` | string | - | Ограничить выдачу трекерами |
| `limit` | int | без ограничения | Максимум результатов, не больше 1000. Если задан только `offset`, лимит - 100 |
| `offset` | int | `0` | Сдвиг выдачи |
| `apikey` | string | - | Ключ API |

### Токены в `query`

При `type` = `tvsearch`/`tv`, `movie`/`moviesearch`, `music` или `book` из строки извлекаются токены в фигурных скобках (регистр не важен), остаток становится текстовым запросом:

| `type` | Поддерживаемые токены |
| --- | --- |
| `tvsearch`, `tv` | `{ImdbId:}`, `{TmdbId:}`, `{TvdbId:}`, `{Season:}`, `{Episode:}`, `{Year:}`, `{Genre:}` (а также `RId`, `TvMazeId`, `DoubanId` - распознаются и отбрасываются) |
| `movie`, `moviesearch` | `{ImdbId:}`, `{TmdbId:}`, `{Year:}`, `{Genre:}` (`DoubanId`, `TraktId` отбрасываются) |
| `music` | `{Artist:}`, `{Album:}`, `{Year:}`, `{Genre:}` |
| `book` | `{Title:}`, `{Author:}`, `{Year:}`, `{Genre:}` |

При `type=search` токены не разбираются.

Если после удаления токенов текста не осталось, запросом становится IMDb ID, затем TMDB ID (`tmdb…`). Запрос, в котором есть только `{TvdbId:}` без названия и других ID, возвращает пустой массив: TVDB ID не поддерживается. Простой текст вида «Русское English 1999» разбирается на название, оригинальное название и год, как в [Jackett API](jackett.md).

Если нет ни запроса, ни `title`/`title_original`, возвращается `[]`.

### Пример

```bash
curl -G "http://localhost:9117/api/v1/search" \
  --data-urlencode "query={ImdbId:tt0903747} {Season:1} {Episode:2}" \
  --data "type=tvsearch" \
  --data "indexerIds=1" \
  -H "X-Api-Key: KEY"
```

### Ответ

```json
[
  {
    "guid": "0123456789abcdef0123456789abcdef01234567",
    "age": 12,
    "ageHours": 290.5,
    "ageMinutes": 17430.2,
    "size": 1610612736,
    "indexerId": 1,
    "indexer": "rutor",
    "title": "Во все тяжкие / Breaking Bad [S01E02] WEB-DL 1080p | [LostFilm].rus",
    "sortTitle": "Во все тяжкие / Breaking Bad [S01E02] WEB-DL 1080p | [LostFilm].rus",
    "publishDate": "2024-03-01T12:00:00Z",
    "downloadUrl": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "magnetUrl": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "infoUrl": "http://rutor.info/torrent/123456",
    "commentUrl": "http://rutor.info/torrent/123456",
    "categories": [
      { "id": 5000, "name": "TV", "subCategories": [] }
    ],
    "protocol": "torrent",
    "infoHash": "0123456789abcdef0123456789abcdef01234567",
    "seeders": 25,
    "leechers": 3,
    "languages": ["rus"],
    "info": {
      "quality": 1080,
      "videotype": "sdr",
      "voices": ["LostFilm"],
      "seasons": [1],
      "types": ["serial"],
      "sizeName": "1.5 GB",
      "name": "Во все тяжкие",
      "originalname": "Breaking Bad",
      "relased": 2008
    }
  }
]
```

| Поле | Тип | Описание |
| --- | --- | --- |
| `guid` | string | Infohash; если его нет - MD5 заголовка |
| `age`, `ageHours`, `ageMinutes` | number | Возраст раздачи в днях (целое), часах и минутах |
| `size` | int | Размер в байтах |
| `indexerId` | int | Всегда `1` |
| `indexer` | string | Трекер-источник (или `CrabIndex (all trackers)`) |
| `title`, `sortTitle` | string | Заголовок; при `torznab.enrichTitles: true` - с суффиксом озвучек |
| `publishDate` | string | Дата публикации (ISO 8601). Пустые даты и даты до 2000 года заменяются текущим временем |
| `downloadUrl` | string | Magnet-ссылка, а если её нет - страница раздачи |
| `magnetUrl` | string | Magnet-ссылка (только если она есть) |
| `infoUrl`, `commentUrl` | string | Страница раздачи (только HTTP-ссылки) |
| `categories` | array | `{id, name, subCategories}`. Если категорий нет - `2000` |
| `protocol` | string | Всегда `torrent` |
| `infoHash` | string | Infohash в нижнем регистре |
| `seeders`, `leechers` | int | Сиды и личеры |
| `ffprobe`, `languages`, `info` | - | Дополнительные поля CrabIndex, как в [Jackett API](jackett.md). Выводятся, только если заполнены |

Поля со значением `null` в ответ не попадают.

## См. также

- [Torznab API](torznab.md)
- [Настройки поиска](../configuration/search.md)
- [Обзор API](overview.md)
