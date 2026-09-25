# Torznab API

Torznab (Newznab-совместимый XML) позволяет подключить CrabIndex как индексер к Sonarr, Radarr, Prowlarr и другим *arr-приложениям. Поиск идёт через тот же конвейер, что и [Jackett API](jackett.md); отличается только формат ответа. Как подключить клиентов, описано на странице [Lampa, Sonarr, Prowlarr](../clients/overview.md).

## Включение

```yaml
torznab:
  enable: true        # по умолчанию true; false → 404 на всех Torznab- и Prowlarr-маршрутах
  enrichTitles: true  # по умолчанию true; добавлять озвучки в <title>
```

Настройки на формат Jackett JSON для Prisma и Lampa не влияют. Подробнее - в [настройках поиска](../configuration/search.md).

## Маршруты

Все три пути работают одинаково и принимают любой HTTP-метод, в том числе с завершающим `/`.

| Путь | Назначение |
| --- | --- |
| `/torznab/api` | Основной Torznab-эндпоинт (все разрешённые трекеры) |
| `/api/v2.0/indexers/{indexer}/results/torznab/api` | Путь в стиле Jackett |
| `/api/v1/indexer/{indexer}/newznab` | Путь в стиле Prowlarr (см. [Prowlarr API](prowlarr.md)) |

`{indexer}` - `all`, `status:healthy`, число (например, `1`) или slug трекера. Slug ограничивает выдачу этим трекером, остальные значения означают все трекеры.

**Доступ:** требуется `apikey`, если он задан в конфигурации (`?apikey=`, `X-Api-Key` или `Authorization: Bearer`). См. [Аутентификация](../authentication.md) и [Матрица доступа](../operations/access-matrix.md).

Пути и имена параметров не зависят от регистра. Значения - зависят: `t` нужно передавать в нижнем регистре (`caps`, `tvsearch`).

:::note[Примечание]
На запрос методом `HEAD` сервер отвечает `200` с пустым телом `application/xml` (так клиенты проверяют доступность).
:::

## GET /torznab/api?t=caps

Возможности индексера. Адрес `url` в `<server>` строится из заголовков `Host` и `X-Forwarded-Proto` запроса.

```bash
curl "http://localhost:9117/torznab/api?t=caps&apikey=KEY"
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<caps>
  <server version="1.0" title="CrabIndex" strapline="Native Torznab API" email="info@localhost" url="http://localhost:9117/torznab/api"/>
  <limits max="1000" default="100"/>
  <searching>
    <search available="yes" supportedParams="q,imdbid"/>
    <tv-search available="yes" supportedParams="q,imdbid,tvdbid,season,ep"/>
    <movie-search available="yes" supportedParams="q,imdbid"/>
  </searching>
  <categories>
    <category id="2000" name="Movies"/>
    <category id="5000" name="TV"/>
    <category id="5070" name="TV/Anime"/>
  </categories>
</caps>
```

## GET /torznab/api?t=indexers

Список индексеров: агрегированный `all` и по одному на каждый разрешённый трекер (`synctrackers` минус `disable_trackers`).

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `configured` | string | - | Пусто или `true` - полный список; любое другое значение - пустой `<indexers>` |

```bash
curl "http://localhost:9117/torznab/api?t=indexers&apikey=KEY"
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<indexers>
  <indexer id="all" configured="true">
    <title>CrabIndex (all trackers)</title>
    <description>Aggregated CrabIndex search across all configured trackers</description>
    <link>https://github.com/sheinices/crabindex</link>
    <language>ru-RU</language>
    <type>public</type>
  </indexer>
  <indexer id="kinozal" configured="true">
    <title>kinozal</title>
    <description>CrabIndex tracker: kinozal</description>
    <link>https://github.com/sheinices/crabindex</link>
    <language>ru-RU</language>
    <type>public</type>
  </indexer>
</indexers>
```

## GET /torznab/api?t=search | tvsearch | moviesearch

Поиск раздач. Любое значение `t`, кроме `caps` и `indexers` (и даже его отсутствие), запускает поиск. Для `t` действуют синонимы `tv` = `tvsearch` и `movie` = `moviesearch`.

### Параметры

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `t` | string | - | `search`, `tvsearch` (`tv`), `moviesearch` (`movie`) |
| `apikey` | string | - | Ключ API |
| `q` | string | - | Поисковый запрос. Поддерживает `tt…`, `kp…`, `tmdb…` так же, как [Jackett API](jackett.md) |
| `query` | string | - | Используется, если `q` не задан |
| `imdbid`, `imdb_id` | string | - | IMDb ID (`tt0133093` или просто цифры), если нет `q`/`query` |
| `tvdbid`, `rid` | string | - | Не поддерживаются. Запрос только с ними (без `q`/`imdbid`) возвращает пустую ленту |
| `title`, `title_original` | string | - | Поля карточки для точного поиска |
| `year` | int | - | Год выпуска |
| `is_serial` | int | - | Тип контента, как в [Jackett API](jackett.md) |
| `cat`, `category`, `category[]`, `categories` | int, список через запятую | - | Категории Newznab |
| `season` | int | - | Номер сезона |
| `ep`, `episode` | int | - | Номер серии |
| `tracker`, `tracker[]` | string | - | Ограничить выдачу трекерами |
| `limit` | int | без ограничения | Максимум результатов, не больше 1000. Если задан только `offset`, лимит - 100 |
| `offset` | int | `0` | Сдвиг выдачи |

Если нет ни запроса, ни `title`, ни `title_original`, возвращается пустая лента `<rss>` без `<item>`.

Значение `t` влияет на фильтрацию и категорию в ответе:

- при `tvsearch`/`tv` каждому `<item>` присваивается категория `5000`, при `moviesearch`/`movie` - `2000`;
- при `tvsearch` и `moviesearch` фильтр по `cat` не применяется;
- при `search` категория берётся из первого значения `cat`, затем из категории раздачи, иначе ставится `2000`.

Фильтр по категориям на сервере включается только при `search.skipCatFilter: false` (по умолчанию `true`). Фильтр по сезону и серии отключается через `search.skipSeasonEpisodeFilter: true`. Подробнее - в [настройках поиска](../configuration/search.md) и в разделе [Поиск](../concepts/search.md).

### Примеры

```bash
# Текстовый поиск
curl "http://localhost:9117/torznab/api?t=search&q=матрица&apikey=KEY"

# Сериал: сезон и серия
curl "http://localhost:9117/torznab/api?t=tvsearch&q=breaking+bad&season=1&ep=1&apikey=KEY"

# Фильм по IMDb
curl "http://localhost:9117/torznab/api?t=moviesearch&imdbid=tt0133093&apikey=KEY"

# Только rutracker через путь в стиле Jackett
curl "http://localhost:9117/api/v2.0/indexers/rutracker/results/torznab/api?t=search&q=матрица&apikey=KEY"
```

### Ответ

```xml
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:torznab="http://torznab.com/schemas/2015/feed">
    <channel>
        <atom:link href="http://localhost:9117/torznab/api" rel="self" type="application/rss+xml" />
        <title>CrabIndex</title>
        <description>Torznab API</description>
        <link>http://localhost:9117/</link>
        <language>en-us</language>
        <category>search</category>
    <item>
        <title>Во все тяжкие / Breaking Bad [S01E01] WEB-DL 1080p | [LostFilm].rus</title>
        <guid isPermaLink="false">0123456789abcdef0123456789abcdef01234567</guid>
        <jackettindexer id="all">rutor</jackettindexer>
        <link>magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567</link>
        <comments>http://rutor.info/torrent/123456</comments>
        <pubDate>Fri, 01 Mar 2024 12:00:00 GMT</pubDate>
        <category>5000</category>
        <size>1610612736</size>
        <enclosure url="magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567" length="1610612736" type="application/x-bittorrent;x-scheme-handler/magnet" />
        <torznab:attr name="magneturl" value="magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567" />
        <torznab:attr name="size" value="1610612736" />
        <torznab:attr name="seeders" value="25" />
        <torznab:attr name="leechers" value="3" />
        <torznab:attr name="peers" value="28" />
        <torznab:attr name="infohash" value="0123456789abcdef0123456789abcdef01234567" />
        <torznab:attr name="downloadvolumefactor" value="1" />
        <torznab:attr name="uploadvolumefactor" value="1" />
        <torznab:attr name="site" value="rutor" />
        <torznab:attr name="category" value="5000" />
        <torznab:attr name="language" value="ru-RU" />
        <torznab:attr name="lang" value="ru" />
        <torznab:attr name="year" value="2008" />
        <torznab:attr name="season" value="1" />
        <torznab:attr name="ep" value="1" />
        <torznab:attr name="episode" value="1" />
    </item>
    </channel>
</rss>
```

| Элемент | Описание |
| --- | --- |
| `<title>` | Заголовок раздачи. При `torznab.enrichTitles: true` и известных озвучках добавляется суффикс `\| [озвучка озвучка].rus` |
| `<guid>` | Infohash в нижнем регистре; если его нет - MD5 заголовка |
| `<jackettindexer>` | Трекер-источник (или `CrabIndex`, если трекер неизвестен) |
| `<link>`, `<enclosure url>` | Magnet-ссылка, а если её нет - ссылка на страницу раздачи |
| `<comments>` | Страница раздачи на трекере (если это HTTP-ссылка) |
| `<pubDate>` | Дата в формате RFC 1123. Пустые даты и даты до 2000 года заменяются текущим временем |
| `<size>` | Размер в байтах (при необходимости вычисляется из текстового размера) |
| `torznab:attr seeders`, `leechers`, `peers` | Сиды, личеры (только если больше 0) и `peers` = сиды + личеры |
| `torznab:attr language`, `lang` | `ru-RU`/`ru`, если в заголовке есть кириллица, иначе `en-US`/`en` для латиницы |
| `torznab:attr year` | Год выпуска, если известен |
| `torznab:attr season`, `ep`, `episode` | Сезон и серия, разобранные из заголовка (`S01E02`, `1x02`, сезонные паки) |

Атрибуты с пустым значением не выводятся.

## См. также

- [Prowlarr API](prowlarr.md) - REST-обнаружение индексера и Search Feed в JSON.
- [Jackett API](jackett.md) - тот же поиск в JSON.
- [Обзор API](overview.md).
