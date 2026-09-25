# Jackett API

Jackett-совместимый JSON-поиск - основной поисковый интерфейс CrabIndex. Его используют Lampa, NUM и другие клиенты, которые умеют работать с Jackett-индексером. Настройка клиентов описана на странице [Lampa, Sonarr, Prowlarr](../clients/overview.md), устройство поискового конвейера - в разделе [Поиск](../concepts/search.md).

Пути и имена query-параметров не зависят от регистра: сервер приводит путь и имена параметров к нижнему регистру, значения не меняет. Подробнее - в [обзоре API](overview.md).

## GET /api/v2.0/indexers/{indexer}/results

Поиск по FileDB с ответом в формате Jackett. Маршрут принимает любой HTTP-метод, обычно используют `GET`. Путь с завершающим `/` тоже работает.

**Доступ:** требуется `apikey`, если он задан в конфигурации. См. [Аутентификация](../authentication.md) и [Матрица доступа](../operations/access-matrix.md).

### Сегмент пути `{indexer}`

| Значение | Поведение |
| --- | --- |
| `all` | Поиск по всем разрешённым трекерам |
| `status:healthy` | То же, что `all` |
| число (например, `1`) | То же, что `all` |
| slug трекера (`rutracker`, `kinozal`, …) | Выдача фильтруется по этому трекеру |

Разрешёнными считаются трекеры из `synctrackers` (если список задан), за вычетом `disable_trackers`. Подробнее - в [Настройка трекеров](../configuration/trackers.md).

### Параметры запроса

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `apikey` | string | - | Ключ API. Вместо него можно передать заголовок `X-Api-Key` или `Authorization: Bearer <ключ>` |
| `query` | string | - | Текстовый запрос или внешний идентификатор: `tt0133093` (IMDb), `kp301` (Кинопоиск), `tmdb603`, `tmdb:603`, ссылка на themoviedb.org. Число из 7-10 цифр считается IMDb ID (`tt` добавляется автоматически). Обрамляющие кавычки отбрасываются |
| `q` | string | - | Используется, если `query` не задан |
| `imdbid`, `imdb_id` | string | - | Используются, если не заданы ни `query`, ни `q` |
| `title` | string | - | Название карточки (обычно русское). Включает точный поиск по карточке |
| `title_original` | string | - | Оригинальное название карточки |
| `year` | int | `0` | Год выпуска. Для фильмов допускается ±1 год, для сериалов - релизы не старше `year - 1` |
| `is_serial` | int | `-1` | Тип контента: `1` - фильм, `2` - сериал, `3` - ТВ-шоу, `4` - документальное, `5` - аниме, `0` - определить по категории, `-1` - без фильтра |
| `category[]`, `category[N]`, `category`, `cat`, `categories` | int, список через запятую | - | Категории Newznab (`2000`, `2010`, `5000`, `5020`, `5070`, `5080`). При `is_serial=0` по ним определяется тип |
| `genres` | string | - | Жанры карточки. Если параметр передан, запрос обрабатывается как поиск по карточке |
| `tracker[]`, `tracker[N]`, `tracker` | string, список через запятую | - | Ограничить выдачу трекерами |
| `season` | int | - | Номер сезона (фильтр по сезону и серии) |
| `ep`, `episode` | int | - | Номер серии |
| `limit` | int | без ограничения | Максимум результатов (не больше 1000). Если задан только `offset`, лимит равен 100 |
| `offset` | int | `0` | Сдвиг выдачи |

Если не заданы ни запрос (`query`/`q`/`imdbid`), ни `title`, ни `title_original`, возвращается пустой список `Results`.

### Как обрабатывается запрос

- **Поиск по карточке.** Если переданы `title`/`title_original`, `is_serial` ≥ 0, категории или `genres`, CrabIndex ищет точные совпадения по названиям и фильтрует их по типу и году.
- **Поиск по внешнему ID.** Запрос вида `tt…`, `kp…` или `tmdb…` без полей карточки разрешается в названия через Alloha (см. [Alloha](../configuration/alloha.md)), затем ищется в FileDB. Если год в запросе не указан, а `alloha.filterByYear: true`, выдача ограничивается годом из Alloha ±1.
- **Свободный текст.** Строка вида «Русское English 1999» или «Русское / English» разбирается на русское и оригинальное название и год. Дополнительно выполняются запросы без года в конце и без токенов `S01E02`, если это включено в [настройках поиска](../configuration/search.md) (`stripTrailingYear`, `stripSeasonEpisode`). Нечёткий поиск подмешивается по правилам `search.mergeV1`.

Результаты разных запросов объединяются по infohash и сортируются по убыванию сидов, затем пиров. Раздачи с одинаковым infohash с разных трекеров склеиваются, если включён `mergeduplicates` (для клиента NUM - `mergenumduplicates`). В этом случае в поле `Tracker` трекеры перечислены через запятую, например `"rutor, kinozal"`.

После поиска применяются фильтры: по категории (если `search.skipCatFilter: false` и запрос не карточный), по году, по сезону и серии (если `search.skipSeasonEpisodeFilter: false`), по трекерам, затем `limit`/`offset`.

:::note[Примечание]
Если в запросе передано `apikey=rus`, в выдаче остаются только раздачи с русской дорожкой, а также спорт, ТВ-шоу и документальные сериалы. Это работает, только когда такой ключ проходит авторизацию: `apikey` в конфигурации не задан или равен `rus`.
:::

### Примеры

```bash
# Текстовый поиск
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=матрица&apikey=KEY"

# Поиск по карточке (как в Prisma и Lampa)
curl -G "http://localhost:9117/api/v2.0/indexers/all/results" \
  --data-urlencode "title=Матрица" \
  --data-urlencode "title_original=The Matrix" \
  --data "year=1999" --data "is_serial=1" --data "apikey=KEY"

# Поиск по IMDb и Кинопоиску
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=tt0133093&apikey=KEY"
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=kp301&apikey=KEY"

# Только один трекер
curl "http://localhost:9117/api/v2.0/indexers/kinozal/results?query=матрица&apikey=KEY"

# Ключ в заголовке
curl -H "X-Api-Key: KEY" \
  "http://localhost:9117/api/v2.0/indexers/all/results?query=матрица"
```

### Ответ

```json
{
  "Results": [
    {
      "Tracker": "rutracker",
      "Details": "https://rutracker.org/forum/viewtopic.php?t=1234567",
      "Title": "Матрица / The Matrix (1999) BDRip 1080p | Дубляж",
      "Size": 10307921510.0,
      "PublishDate": "2024-03-01T12:00:00Z",
      "Category": [2000],
      "CategoryDesc": "Movies",
      "Seeders": 50,
      "Peers": 10,
      "MagnetUri": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
      "languages": ["rus"],
      "info": {
        "quality": 1080,
        "videotype": "sdr",
        "voices": ["Дубляж"],
        "types": ["movie"],
        "sizeName": "9.6 GB",
        "name": "Матрица",
        "originalname": "The Matrix",
        "relased": 1999
      }
    }
  ],
  "jacred": true
}
```

| Поле | Тип | Описание |
| --- | --- | --- |
| `Results` | array | Найденные раздачи |
| `Results[].Tracker` | string | Slug трекера; после склейки дубликатов - несколько через запятую |
| `Results[].Details` | string | Ссылка на страницу раздачи (только если URL начинается с `http`) |
| `Results[].Title` | string | Заголовок раздачи |
| `Results[].Size` | number | Размер в байтах |
| `Results[].PublishDate` | string | Дата создания записи (ISO 8601, UTC) |
| `Results[].Category` | int[] | Категории Newznab: `movie` → 2000, `serial` → 5000, `documovie`/`docuserial` → 5080, `tvshow` → 5020 и 2010, `anime` → 5070 |
| `Results[].CategoryDesc` | string | Описание категории (`Movies`, `TV`, `TV/Documentary`, `TV/Foreign`, `TV/Anime`) |
| `Results[].Seeders` | int | Сиды |
| `Results[].Peers` | int | Пиры (личеры) |
| `Results[].MagnetUri` | string | Magnet-ссылка |
| `Results[].ffprobe` | array | Потоки ffprobe, если их собрал модуль [Tracks](../concepts/tracks.md) |
| `Results[].languages` | string[] | Языки аудио (`rus`, `ukr`, …) |
| `Results[].info` | object | Разобранные метаданные: `quality` (480/720/1080/2160), `videotype` (`sdr`/`hdr`), `voices`, `seasons`, `types`, `sizeName`, `name`, `originalname`, `relased` (год) |
| `jacred` | bool | Всегда `true`. Поле оставлено для совместимости: Lampa и другие клиенты по нему определяют тип сервера |

Пустые строковые поля в ответ не попадают. Для клиента NUM (определяется по User-Agent, если в запросе нет `is_serial`) поля `ffprobe`, `languages` и `info` не заполняются.

## GET /api/v2.0/indexers

Список индексеров в формате Jackett: агрегированный `all` и по одному на каждый разрешённый трекер. Маршрут принимает любой метод и не зависит от `torznab.enable`.

**Доступ:** требуется `apikey`, если он задан.

Параметров нет.

```bash
curl "http://localhost:9117/api/v2.0/indexers?apikey=KEY"
```

```json
[
  {
    "id": "all",
    "name": "CrabIndex (all trackers)",
    "description": "Aggregated CrabIndex search across all configured trackers",
    "type": "public",
    "configured": true,
    "link": "https://github.com/sheinices/crabindex"
  },
  {
    "id": "kinozal",
    "name": "kinozal",
    "description": "CrabIndex tracker: kinozal",
    "type": "public",
    "configured": true,
    "link": "https://github.com/sheinices/crabindex"
  }
]
```

Трекеры идут в алфавитном порядке. Список строится из `synctrackers`, а если он не задан - из всех 25 встроенных трекеров; `disable_trackers` из него исключаются.

## См. также

- [Torznab API](torznab.md) - тот же поиск в XML для Sonarr и Radarr.
- [Нативный API](native.md) - прямой поиск по FileDB.
- [Настройки поиска](../configuration/search.md).
