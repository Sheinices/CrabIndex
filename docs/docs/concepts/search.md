# Поиск

Все поисковые API CrabIndex работают поверх одной FileDB и одного поискового конвейера. Разница только в формате запроса и ответа.

| API | Адрес | Формат | Типичные клиенты |
| --- | --- | --- | --- |
| Jackett | `GET /api/v2.0/indexers/all/results` | JSON | Prisma, Lampa, Stremio-аддоны, AIOStreams |
| Torznab | `GET /torznab/api?t=search` | XML (RSS) | Sonarr, Radarr, Prowlarr |
| Prowlarr | `GET /api/v1/indexer/1/newznab`, `GET /api/v1/search` | XML / JSON | Prowlarr |
| Собственный | `GET /api/v1.0/torrents` | JSON | Веб-интерфейс, свои интеграции |

Подробные параметры - в справочнике: [Jackett](../api-reference/jackett.md), [Torznab](../api-reference/torznab.md), [Prowlarr](../api-reference/prowlarr.md), [Нативный API](../api-reference/native.md).

## Режимы поиска

Конвейер выбирает режим по тому, что прислал клиент.

### Карточка (card)

Клиент присылает поля карточки фильма: `title`, `title_original`, `year`, `is_serial` (так делают Prisma и Lampa). CrabIndex ищет бакеты FileDB с **точным** совпадением нормализованных названий, учитывая год и тип (фильм / сериал / аниме). Если точных совпадений нет, поиск повторяется по вариантам текстового запроса (см. ниже).

### Текстовый (fuzzy)

Клиент присылает только `query` (или `Query`). CrabIndex:

1. строит варианты запроса: исходный, без года в конце (`stripTrailingYear`), без `S01` / `S01E01` (`stripSeasonEpisode`), и их комбинацию;
2. ищет каждый вариант через индекс токенов FastDB - по вхождению в названия бакетов;
3. при `search.mergeV1: auto` или `true` добавляет результаты собственного API (до `maxV1Pairs` пар «название / альтернативное название»).

Строки вида «Русское название English Title 1999» автоматически разбираются на русское название, оригинальное название и год.

### Поиск по ID

Если `query` - это идентификатор внешней базы, CrabIndex сначала узнаёт названия через [Alloha TV API](../configuration/alloha.md), а затем ищет их в FileDB:

| Формат | Пример |
| --- | --- |
| IMDb | `tt0133093` |
| Кинопоиск | `kp301` |
| TMDB | `tmdb603`, `tmdb:603` |
| Ссылка TMDB | `https://www.themoviedb.org/movie/603-the-matrix` |

Если клиент не передал год, а `alloha.filterByYear: true`, результаты фильтруются по году из Alloha ±1. Тип контента из Alloha (фильм / сериал / аниме) используется как фильтр, если клиент не передал категории. Если Alloha выключена, не настроена или ничего не нашла, строка ищется как обычный текст.

```bash
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=tt0133093&apikey=KEY"
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=kp301&apikey=KEY"
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=tmdb603&apikey=KEY"
```

:::note[Примечание]
Голый `query=tt…` - это поиск по ID, а не поиск карточки. Карточный режим включается только когда клиент сам присылает `title` / `title_original`.
:::

## Объединение и фильтры

- **Слияние по infohash.** Результаты всех вариантов запроса объединяются по infohash (при его отсутствии - по хешу названия и magnet). Из дублей остаётся лучшее число сидов и пиров, озвучки объединяются.
- **Дубли между трекерами** (`mergeduplicates`, `mergenumduplicates`). Одна и та же раздача (одинаковый infohash) на разных трекерах превращается в одну запись с объединённым списком announce-трекеров в magnet. `mergenumduplicates` действует для запросов, пришедших от клиента NUM, `mergeduplicates` - для остальных.
- **Сезон и серия.** Torznab/Jackett-результаты фильтруются по `season` / `ep`, если не включён `search.skipSeasonEpisodeFilter`.
- **Категории.** Фильтр по `cat` / `Category[]` отключён по умолчанию (`search.skipCatFilter: true`) - Prisma и Lampa фильтруют сами.
- **Отключённые трекеры.** Раздачи трекеров из `disable_trackers` не попадают в выдачу.

## Ограничения чтения

Нечёткий поиск по короткому слову может задеть тысячи бакетов. Параметр `maxreadfile` ограничивает число читаемых за один запрос бакетов (по умолчанию `200`). Ограничение снимается, только если вся база держится в памяти (`evercache.enable: true` и `evercache.validHour: 0`). См. [FileDB](filedb.md).

## Примеры

```bash
# Текстовый поиск фильма
curl "http://localhost:9117/api/v2.0/indexers/all/results?query=матрица+1999&apikey=KEY"

# Карточка, как её присылают Prisma и Lampa
curl -sG "http://localhost:9117/api/v2.0/indexers/all/results" \
  --data-urlencode "title=Матрица" \
  --data-urlencode "title_original=The Matrix" \
  --data-urlencode "year=1999" \
  --data-urlencode "is_serial=1" \
  --data-urlencode "apikey=KEY"

# Собственный API с фильтром по трекеру и сортировкой по сидам
curl "http://localhost:9117/api/v1.0/torrents?search=матрица&tracker=rutracker&sort=sid&apikey=KEY"

# Torznab
curl "http://localhost:9117/torznab/api?t=search&q=matrix&apikey=KEY"
```

Настройки поиска описаны в разделе [Конфигурация поиска](../configuration/search.md).
