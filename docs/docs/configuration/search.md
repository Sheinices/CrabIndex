# Поиск

Поведение поискового конвейера задают блоки `search` и `torznab`, флаги `mergeduplicates` / `mergenumduplicates` и лимит `maxreadfile`. Как устроен сам поиск - см. [Поиск](../concepts/search.md).

## Блок search

Общий для Jackett JSON, Torznab XML и Prowlarr.

```yaml
search:
  mergeV1: auto                  # false | auto | true
  maxV1Pairs: 4
  v1Sort: sid
  stripTrailingYear: true
  stripSeasonEpisode: true
  skipSeasonEpisodeFilter: false
  skipCatFilter: true
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `mergeV1` | `auto` | Добавлять ли к результатам выдачу собственного API (нечёткий поиск по названию). `false` - никогда; `auto` - только для текстовых запросов, не для карточки; `true` - всегда, без ограничения числа пар |
| `maxV1Pairs` | `4` | Сколько пар «название / альтернативное название» пробовать при `mergeV1: auto` |
| `v1Sort` | `sid` | Сортировка выдачи собственного API: `sid` (сиды), `pir` (пиры), `size`, `create`, `update`. Используется и при поиске по IMDb / Кинопоиску |
| `stripTrailingYear` | `true` | Добавлять вариант запроса без года в конце («Матрица 1999» → «Матрица») |
| `stripSeasonEpisode` | `true` | Добавлять вариант запроса без `S01` / `S01E01` |
| `skipSeasonEpisodeFilter` | `false` | Не фильтровать результаты по сезону и серии на сервере - полезно, когда клиент (например, AIOStreams) фильтрует сам |
| `skipCatFilter` | `true` | Не фильтровать по `cat` / `Category[]` - клиент получает все категории (Prisma и Lampa фильтруют сами) |

Варианты запроса (без года, без сезона) используются в текстовом режиме, а в карточном - только когда точное совпадение ничего не нашло.

## Объединение дублей

```yaml
mergeduplicates: true
mergenumduplicates: true
```

Одна и та же раздача (одинаковый infohash) с разных трекеров объединяется в одну запись; announce-адреса всех источников добавляются в magnet. `mergeduplicates` действует для обычных клиентов, `mergenumduplicates` - для запросов клиента NUM. Выключите, если хотите видеть каждую копию отдельно.

## Блок torznab

```yaml
torznab:
  enable: true
  enrichTitles: true
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `enable` | `true` | `false` - `/torznab/api` и эндпоинты Prowlarr отвечают `404`. Jackett JSON не затрагивается |
| `enrichTitles` | `true` | Добавлять озвучки в `<title>` Torznab-выдачи и в выдачу Prowlarr |

## Лимит чтения

```yaml
maxreadfile: 200
```

Максимум бакетов FileDB, которые читает один нечёткий поиск. Больше - полнее выдача по коротким запросам, но выше нагрузка на диск. Не действует, если включён `evercache` с `validHour: 0` (вся база в памяти). См. [FileDB](../concepts/filedb.md).

## Отключённые трекеры

Раздачи трекеров из `disable_trackers` исключаются из выдачи. См. [Трекеры](trackers.md#отключение-трекеров).

## Готовые профили

**Prisma и Lampa (по умолчанию)** - оставьте значения по умолчанию.

**AIOStreams / Stremio** - клиент сам фильтрует сезон и серию:

```yaml
search:
  skipSeasonEpisodeFilter: true
  stripSeasonEpisode: true
  skipCatFilter: true
```

**Sonarr / Radarr** - серверная фильтрация по категориям и сезону:

```yaml
search:
  skipCatFilter: false
  skipSeasonEpisodeFilter: false
```

**Отладка** - всегда добавлять нечёткий поиск:

```yaml
search:
  mergeV1: true
```

Поиск по IMDb / Кинопоиску / TMDB настраивается в блоке [alloha](alloha.md).
