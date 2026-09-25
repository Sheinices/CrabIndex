# Нативный API

Нативный JSON API работает с FileDB напрямую: возвращает записи в том виде, в каком они хранятся, без Jackett-обёртки и без склейки дубликатов. Его использует веб-интерфейс CrabIndex, он подходит и для собственных интеграций. Как устроено хранилище, описано в разделе [FileDB](../concepts/filedb.md), поиск в целом - в разделе [Поиск](../concepts/search.md).

**Доступ:** все маршруты этой страницы требуют `apikey`, если он задан в конфигурации (`?apikey=`, `X-Api-Key` или `Authorization: Bearer`). См. [Аутентификация](../authentication.md) и [Матрица доступа](../operations/access-matrix.md).

Пути и имена query-параметров не зависят от регистра, значения передаются как есть.

## GET /api/v1.0/torrents

Поиск раздач в FileDB. Маршрут принимает любой HTTP-метод.

### Параметры

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `search` | string | - | Название или внешний ID: `tt…` (IMDb), `kp…` (Кинопоиск), `tmdb…`, ссылка на themoviedb.org. Пустой запрос или запрос из одного символа возвращает `[]` |
| `altname` | string | - | Альтернативное название (например, оригинальное) |
| `exact` | bool | `false` | `true` - только точное совпадение названия или оригинального названия. Для внешних ID включается автоматически |
| `type` | string | - | Тип раздачи: `movie`, `serial`, `multfilm`, `multserial`, `anime`, `documovie`, `docuserial`, `tvshow`, `sport` |
| `sort` | string | - | Сортировка по убыванию: `sid` (сиды), `pir` (пиры), `size`, `create` (дата создания), `update` (дата обновления). Без параметра порядок не гарантирован |
| `tracker` | string | - | Трекер или несколько через запятую (`rutor,kinozal`) |
| `voice` | string | - | Озвучка, точное совпадение с элементом `voices` (например, `LostFilm`) |
| `videotype` | string | - | `sdr` или `hdr` |
| `relased` | int | - | Год выпуска, точное совпадение |
| `quality` | int | - | Качество: `480`, `720`, `1080`, `2160` |
| `season` | int | - | Номер сезона, должен быть в `seasons` |
| `apikey` | string | - | Ключ API |

Как идёт поиск:

- Название приводится к поисковому ключу: нижний регистр, только буквы и цифры, `ё` → `е`.
- Без `exact` берутся все записи, ключ которых содержит `search` или `altname`. Если `evercache` не работает в постоянном режиме (`enable: true`, `validHour: 0`), читается не больше `maxreadfile` групп FileDB.
- С `exact=true` название или оригинальное название записи должно совпасть с ключом полностью.
- Внешний ID разрешается через Alloha (см. [Alloha](../configuration/alloha.md)) в название и альтернативное название. Если `type` не задан, берётся тип из Alloha. Если не задан `relased`, а `alloha.filterByYear: true`, остаются записи с годом Alloha ±1 или без года.
- Записи без типа и записи трекеров, которые не входят в `synctrackers` или входят в `disable_trackers`, не возвращаются.
- В ответе не больше 2000 записей.

### Примеры

```bash
# Текстовый поиск, сортировка по сидам
curl "http://localhost:9117/api/v1.0/torrents?search=матрица&sort=sid&apikey=KEY"

# Точный поиск по двум названиям
curl -G "http://localhost:9117/api/v1.0/torrents" \
  --data-urlencode "search=Матрица" \
  --data-urlencode "altname=The Matrix" \
  --data "exact=true" --data "apikey=KEY"

# По IMDb ID, только 1080p с rutracker
curl "http://localhost:9117/api/v1.0/torrents?search=tt0133093&quality=1080&tracker=rutracker&apikey=KEY"

# Второй сезон сериала с озвучкой LostFilm
curl "http://localhost:9117/api/v1.0/torrents?search=во+все+тяжкие&type=serial&season=2&voice=LostFilm&apikey=KEY"
```

### Ответ

```json
[
  {
    "tracker": "rutracker",
    "url": "https://rutracker.org/forum/viewtopic.php?t=1234567",
    "title": "Матрица / The Matrix (1999) BDRip 1080p",
    "size": 10307921510.0,
    "sizeName": "9.6 GB",
    "createTime": "2024-03-01T12:00:00Z",
    "updateTime": "2024-06-15T08:30:00Z",
    "sid": 50,
    "pir": 10,
    "magnet": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "name": "Матрица",
    "originalname": "The Matrix",
    "relased": 1999,
    "videotype": "sdr",
    "quality": 1080,
    "voices": ["Дубляж"],
    "seasons": [],
    "types": ["movie"]
  }
]
```

| Поле | Тип | Описание |
| --- | --- | --- |
| `tracker` | string | Slug трекера |
| `url` | string | Страница раздачи (только если URL начинается с `http`) |
| `title` | string | Заголовок раздачи |
| `size` | number | Размер в байтах |
| `sizeName` | string | Размер текстом, как на трекере |
| `createTime`, `updateTime` | string | Даты создания и последнего обновления записи (ISO 8601, UTC) |
| `sid`, `pir` | int | Сиды и пиры |
| `magnet` | string | Magnet-ссылка |
| `name`, `originalname` | string | Название и оригинальное название |
| `relased` | int | Год выпуска (`0`, если неизвестен) |
| `videotype` | string | `sdr` или `hdr` |
| `quality` | int | `480`, `720`, `1080` или `2160` |
| `voices` | string[] | Озвучки |
| `seasons` | int[] | Сезоны |
| `types` | string[] | Типы раздачи |

Пустые строковые поля в ответ не попадают. Если ничего не найдено, возвращается `[]`.

## GET /api/v1.0/trackers

Список трекеров, по которым работает поиск. Только метод `GET`, параметров нет.

Список строится из `synctrackers`, а если он не задан - из всех 25 встроенных трекеров; `disable_trackers` исключаются. Значения без повторов и в алфавитном порядке.

```bash
curl "http://localhost:9117/api/v1.0/trackers?apikey=KEY"
```

```json
["anibelka", "anidub", "anifilm", "aniliberty", "animelayer", "anistar", "baibako", "bitru", "kinozal", "knaben"]
```

## GET /api/v1.0/qualitys

Сводка по доступным качествам, типам и языкам для каждого релиза: группировка по паре «название:оригинальное название», внутри - по году. Маршрут принимает любой HTTP-метод.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `name` | string | - | Название |
| `originalname` | string | - | Оригинальное название |
| `type` | string | - | Учитывать только раздачи этого типа (`movie`, `serial`, …) |
| `page` | int | `1` | Номер страницы |
| `take` | int | `1000` | Размер страницы. `-1` - вернуть всё без сортировки и пагинации |
| `apikey` | string | - | Ключ API |

Если не заданы ни `name`, ни `originalname`, ответ - `{}`. Учитываются группы FileDB, ключ которых содержит поисковый ключ `name` или `originalname`; группы берутся от самой свежей, их число ограничено `maxreadfile` (если `evercache` не работает в постоянном режиме). Раздачи без типа, без года и спортивные пропускаются. Релизы сортируются по дате последнего обновления, от новых к старым.

```bash
curl -G "http://localhost:9117/api/v1.0/qualitys" \
  --data-urlencode "name=Матрица" \
  --data-urlencode "originalname=The Matrix" \
  --data "type=movie" --data "apikey=KEY"
```

```json
{
  "матрица:thematrix": {
    "1999": {
      "qualitys": [1080, 2160, 720],
      "types": ["movie"],
      "languages": ["rus", "eng"],
      "createTime": "2012-05-10T09:00:00Z",
      "updateTime": "2024-06-15T08:30:00Z"
    }
  }
}
```

| Поле | Тип | Описание |
| --- | --- | --- |
| ключ верхнего уровня | string | Поисковые ключи названия и оригинального названия через `:` |
| ключ второго уровня | string | Год выпуска |
| `qualitys` | int[] | Все встреченные качества |
| `types` | string[] | Все встреченные типы |
| `languages` | string[] | Языки аудио (с учётом данных [Tracks](../concepts/tracks.md), если модуль включён) |
| `createTime` | string | Самая ранняя дата создания среди раздач |
| `updateTime` | string | Самая поздняя дата обновления среди раздач |

## См. также

- [Идентификация (/api/v1.0/conf)](conf.md) - проверка сервера и ключа.
- [Jackett API](jackett.md) - поиск с объединением дубликатов и Jackett-форматом.
- [Обзор API](overview.md).
