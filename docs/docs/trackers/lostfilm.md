# LostFilm

![](/img/trackers/lostfilm.ico)

LostFilm (slug `lostfilm`, около 18 тысяч раздач) - студия озвучки сериалов и фильмов. CrabIndex читает ленту новинок `/new/`, для каждой серии открывает страницу серии, получает V-страницу со ссылками на торренты и скачивает `.torrent`-файлы. В базу попадают только качества **1080p** и **2160p**: каждое качество - отдельная запись, URL которой содержит качество во фрагменте (`…/episode_3/#1080p`).

Фильмы из ленты обрабатываются так же (страница фильма → V-страница → 1080p/2160p). Для полных сезонов есть отдельный маршрут `ParseSeasonPacks`.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Lostfilm` |
| `host` | `https://www.lostfilm.tv` |
| `alias` | не используется |
| `reqMinute` | `8` (задержка `parseDelay` = 7000 мс) |
| Авторизация | обязательна, только `cookie` (логин/пароль не поддерживаются) |
| Кодировка | UTF-8 |
| Магнет-ссылки | строятся из скачанных `.torrent` (1080p и 2160p) |
| Учитывает `disable_trackers` | нет - cron-маршруты работают и для отключённого трекера |

## Конфигурация

```yaml
Lostfilm:
  host: https://www.lostfilm.tv
  useproxy: false
  reqMinute: 8
  log: true
  cookie: "lf_loyal_person=0; lf_session=СЕССИЯ; lf_udv=UDV; PHPSESSID=PHPSESSID"
```

Без `cookie` все parse-маршруты сразу возвращают `auth` и ничего не делают.

### Как получить cookie

1. Войдите в аккаунт на `https://www.lostfilm.tv` в браузере.
2. Откройте DevTools (F12) → Application (Storage) → Cookies → `www.lostfilm.tv`.
3. Скопируйте значения `lf_loyal_person`, `lf_session`, `lf_udv` и `PHPSESSID`.
4. Соберите их в одну строку вида `имя=значение; имя=значение; …`.

:::warning[Внимание]
Сессия протухает при выходе из аккаунта или по истечении срока. Если в логе появляются сообщения `cookie expired?` или `no inner-box--link`, обновите cookie.
:::

## Cron-маршруты

Пути и имена параметров нечувствительны к регистру. Все маршруты принимают GET и POST.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/lostfilm/parse` | - | `ok`, `work`, `auth` | Первая страница `/new/` (свежие серии и фильмы) |
| `/cron/lostfilm/ParsePages` | `pageFrom` (по умолчанию 1, минимум 1), `pageTo` (по умолчанию 1; если меньше `pageFrom` - равен ему) | `ok`, `work`, `auth` | Страницы `/new/page_N` в диапазоне. `pageTo` обрезается до реального числа страниц ленты (не больше 100) |
| `/cron/lostfilm/ParseSeasonPacks` | `series` (обязателен) - slug сериала из URL, например `The_Last_of_Us` | `ok`, `work`, `auth`, `series required`, `empty`, `no relased` | Разбирает `/series/{series}/seasons/` и добавляет полные сезоны (`e=999`) в 1080p/2160p |
| `/cron/lostfilm/VerifyPage` | `series` (необязателен) - фильтр по сериалу | JSON | Диагностика: скачивает `/new/` и показывает, что извлёк парсер. В базу ничего не пишет |
| `/cron/lostfilm/Stats` | - | JSON | Статистика записей LostFilm в FileDB |

`parse`, `ParsePages` и `ParseSeasonPacks` используют одну блокировку: пока выполняется один из них, остальные сразу отвечают `work`.

Пример ответа `VerifyPage` (значения условные):

```json
{
  "ok": true,
  "url": "https://www.lostfilm.tv/new/",
  "filteredBy": "The_Last_of_Us",
  "count": 1,
  "items": [
    {
      "title": "Одни из нас / The Last of Us",
      "dateStr": "20.04.2025",
      "relased": 2023,
      "url": "https://www.lostfilm.tv/series/The_Last_of_Us/season_2/episode_2/",
      "source": "episode_links"
    }
  ]
}
```

Поле `filteredBy` есть только при заданном `series`. При пустом ответе или ответе без `LostFilm.TV` возвращается `{"error": "empty", "url": "…"}`.

Пример ответа `Stats`:

```json
{
  "total": 18234,
  "withMagnet": 18230,
  "withoutMagnet": 4,
  "keysCount": 1480,
  "keys": ["..."],
  "keysMore": 1430
}
```

`keys` содержит не более 50 ключей FileDB, `keysMore` - сколько ключей не поместилось. Маршрут читает всю базу, на больших инсталляциях он работает заметное время.

## Рекомендуемое расписание

```crontab
# lostfilm-parse -> /cron/lostfilm/parse (max_time=900s)
11,26,41,56 * * * *  /opt/crabindex/Data/run-job.sh lostfilm-parse http://127.0.0.1:9117/cron/lostfilm/parse 900
```

Каждые 15 минут читается первая страница ленты - этого достаточно, чтобы не пропускать новинки. `ParsePages` и `ParseSeasonPacks` запускаются вручную: для первичного наполнения базы и для добавления полных сезонов.

## Файлы в Data/temp

Нет. У LostFilm нет очереди страниц и цикла ParseAll.

## Особенности и ограничения

- Сохраняются только 1080p и 2160p; SD и 720p пропускаются.
- Для каждой серии нужно несколько запросов (страница серии, `v_search.php`, V-страница, скачивание `.torrent`), поэтому полный проход через `ParsePages` медленный. Пауза между скачиваниями торрентов - `parseDelay`, но не больше 2 секунд.
- Если для URL в базе уже есть магнет, торрент повторно не скачивается.
- `alias` не используется: все запросы идут на `host`.
- `sid` для записей LostFilm всегда равен 1 - сайт не отдаёт статистику сидов.

## Диагностика

```bash
# Ручной запуск
curl "http://127.0.0.1:9117/cron/lostfilm/parse"

# Первичное наполнение: страницы 1-20 ленты
curl "http://127.0.0.1:9117/cron/lostfilm/ParsePages?pageFrom=1&pageTo=20"

# Полные сезоны сериала
curl "http://127.0.0.1:9117/cron/lostfilm/ParseSeasonPacks?series=The_Last_of_Us"

# Что видит парсер на /new/ (проверка cookie)
curl "http://127.0.0.1:9117/cron/lostfilm/VerifyPage"

# Статистика в FileDB
curl "http://127.0.0.1:9117/cron/lostfilm/Stats"

tail -f Data/log/lostfilm.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
