# Cron

Cron API - HTTP-маршруты, которые запускают парсеры трекеров и служебные задачи. CrabIndex не планирует обход трекеров сам: расписание задаёт системный `cron` (или любой другой планировщик), который вызывает эти URL. Готовый crontab лежит в `Data/crontab`, его установка описана на странице [Cron](../deployment/cron.md).

## Доступ

Все пути `/cron/*` и `/jsondb/*` относятся к административным:

- без ключа они доступны только из LAN и с localhost, причём при прямом подключении, без обратного прокси;
- извне и через прокси нужен `devkey`: заголовок `X-Dev-Key` или `?devkey=`;
- если `devkey` не задан, запрос извне получает `403`, с неверным ключом - `401`.

Те же маршруты доступны из [админ-панели](../admin.md) как `{admin.path}/api/cron/*` и `{admin.path}/api/jsondb/*` - там вместо `devkey` действует сессия панели. Раздел **Трекеры** панели запускает действия трекеров, **Задачи** - `ParseAllStatus` и `ResumeParseAll`, **Обслуживание** - проверку FileDB, `jsondb/save` и прогрев Cloudflare.

```bash
# С сервера
curl "http://127.0.0.1:9117/cron/rutor/parse"

# Через обратный прокси
curl -H "X-Dev-Key: YOUR_DEV_KEY" "https://crabindex.example.com/cron/rutor/parse"
```

Пути и имена параметров не зависят от регистра: `/cron/rutor/ParseAllTask` и `/cron/rutor/parsealltask` - один маршрут. Большинство маршрутов принимают `GET` и `POST`, а часть трекеров (aniliberty, anifilm, leproduction, viruseproject, rudub, subsplease) только `GET`. Используйте `GET`.

Каждый вызов `/cron/*` записывается в консольный журнал строкой вида `cron: [12:00:01] rutor/parse 1.5s 200`. Быстрые успешные вызовы (короче `logging.cronSkipFastMs`, по умолчанию 100 мс) пишутся на уровне Debug.

## Типы задач трекеров

| Маршрут | Как выполняется | Ответ |
| --- | --- | --- |
| `/cron/{tracker}/parse` | Синхронно, пока идёт обход. Один запуск на трекер: повторный вызов получает `work` | Текстовый лог (`{cat} - {page} / True`...), `ok` или `work`. Если трекер в `disable_trackers` и маршрут это учитывает, ответ `disabled` |
| `/cron/{tracker}/UpdateTasksParse` | В фоне. Перестраивает карту страниц (`Data/temp/{tracker}_taskParse.json`) по живому пейджеру, лишние хвосты удаляет. Лимит 30 минут (у kinozal 2 часа) | Сразу `ok`, `work` или `disabled` |
| `/cron/{tracker}/ParseAllTask` | В фоне. Обходит все страницы карты, которые ещё не пройдены в текущем цикле. Новый цикл начинается, только когда в старом не осталось страниц (pending = 0). Если прогресса нет 45 минут, задача прерывается | Сразу `ok`, `work` или `disabled` |
| `/cron/{tracker}/ParseLatest?pages=5` | Синхронно. Первые `pages` страниц каждого раздела карты. Не запускается одновременно с `ParseAllTask` и `UpdateTasksParse` | Список `{cat} - {page}`, `ok` или `work` |

Страница, которая не загрузилась 3 раза подряд, в текущем цикле `ParseAllTask` пропускается (в логе трекера: `ParseAll skip slot page=… after 3 failures`). В следующем цикле её пробуют снова. `ParseAllTask` и `ParseLatest` уступают часовому `parse`: между страницами они ждут, пока закончится `parse` того же трекера.

Задачи `UpdateTasksParse`, `ParseAllTask` и `ParseLatest` есть у rutracker, kinozal, rutor, nnmclub, megapeer, torrentby, toloka, anibelka, korsars и ultradox. У остальных трекеров только `parse` и свои маршруты.

## Маршруты по трекерам

| Трекер | Маршруты | Параметры |
| --- | --- | --- |
| [rutracker](../trackers/rutracker.md) | `parse`, `UpdateTasksParse`, `ParseAllTask`, `ParseLatest` | см. страницу трекера |
| [kinozal](../trackers/kinozal.md), [rutor](../trackers/rutor.md), [nnmclub](../trackers/nnmclub.md), [megapeer](../trackers/megapeer.md), [torrentby](../trackers/torrentby.md) | `parse`, `UpdateTasksParse`, `ParseAllTask`, `ParseLatest` | `parse?page=0`, `ParseLatest?pages=5` |
| [toloka](../trackers/toloka.md), [anibelka](../trackers/anibelka.md), [korsars](../trackers/korsars.md), [ultradox](../trackers/ultradox.md) | `parse`, `UpdateTasksParse`, `ParseAllTask`, `ParseLatest` | см. страницы трекеров |
| [bitru](../trackers/bitru.md) | `parse`, `backfill`, `ParseFromDate` | `backfill?pages=20` (1..50) |
| [knaben](../trackers/knaben.md) | `parse`, `backfill`, `BackfillStatus` | `backfill?pages=10&size=300&reset=` |
| [subsplease](../trackers/subsplease.md) | `parse`, `ParseShows`, `ParseShowStatus` | `parse?pages=2`, `ParseShows?limit=50` |
| [lostfilm](../trackers/lostfilm.md) | `parse`, `ParsePages`, `ParseSeasonPacks`, `VerifyPage`, `Stats` | `ParsePages?pageFrom=&pageTo=`, `ParseSeasonPacks?series=` |
| [animelayer](../trackers/animelayer.md) | `parse`, `TakeLogin` | `parse?parseFrom=&parseTo=` |
| [rudub](../trackers/rudub.md) | `parse` | `limit_page` (1..100), `parseFrom`, `parseTo` |
| [anistar](../trackers/anistar.md), [leproduction](../trackers/leproduction.md), [viruseproject](../trackers/viruseproject.md) | `parse` | `limit_page` |
| [anifilm](../trackers/anifilm.md) | `parse` | `fullparse=true` |
| [anidub](../trackers/anidub.md), [baibako](../trackers/baibako.md) | `parse` | `parseFrom`, `parseTo` |
| [aniliberty](../trackers/aniliberty.md) | `parse` | `parseFrom`, `parseTo` |
| [mazepa](../trackers/mazepa.md), [selezen](../trackers/selezen.md) | `parse` | - |

Полный путь строится так: `/cron/{tracker}/{маршрут}`, например `/cron/bitru/backfill?pages=20`. Параметры, ответы и рекомендуемое расписание описаны на страницах трекеров.

## GET /cron/cloudflare/Warmup

Прогревает сессию FlareSolverr: открывает URL в браузере, чтобы пройти проверку Cloudflare заранее, до обхода трекера. Решение проверки занимает до нескольких минут и нагружает CPU, поэтому в crontab прогрев стоит за 5 минут до часового `rutracker/parse` и повторяется каждые 20 минут, чтобы поддерживать сессию.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `url` | string | `https://rutracker.org/forum/tracker.php?nm=` | Какую страницу открыть |

```bash
curl "http://127.0.0.1:9117/cron/cloudflare/Warmup"
```

```json
{ "ok": true, "host": "rutracker.org", "length": 184233, "tookSeconds": 11.4 }
```

| Поле | Описание |
| --- | --- |
| `ok` | Получен непустой HTML |
| `host` | Хост прогретого URL |
| `length` | Длина полученной страницы в символах |
| `tookSeconds` | Длительность с точностью до 0,1 с |

Если прогрев успешен, хост помечается как защищённый Cloudflare, и дальнейшие запросы к нему сразу идут через FlareSolverr или cffetch. Если прогрев не удался, пометка снимается. Настройка описана в разделе [FlareSolverr и cffetch](../configuration/flaresolverr.md).


## Cloudflare: состояние и управление

Эти маршруты использует раздел **FlareSolverr** админ-панели (через `{admin.path}/api/cron/cloudflare/...`).

| Маршрут | Описание |
| --- | --- |
| `GET /cron/cloudflare/status` | Состояние FlareSolverr (`solver`: доступен ли, версия, сессии в браузере) и cffetch, настройки, открытые сессии, защищённые хосты и статистика: `stats.hosts` (по каждому сайту запросы через браузер, успехи, ошибки по причинам `tabCrashed`, `browserTimeouts`, `challengeFailed`, `sessionErrors`, `unreachable`, `pageFailed`, `otherErrors`, сессии, быстрый путь, среднее и максимальное время) и `stats.recentErrors` (до 100 последних ошибок) |
| `POST /cron/cloudflare/sessions/close[?host=]` | Закрыть сессии браузера: все или одного сайта. Сессии, занятые запросом, пропускаются. Ответ `{"ok": true, "closed": N, "busy": M}` |
| `POST /cron/cloudflare/pause?value=true\|false` | Приостановить (`true`) или снова включить (`false`) использование браузера до перезапуска службы. Конфиг не меняется; при паузе закрываются все сессии |
| `POST /cron/cloudflare/stats/reset` | Обнулить статистику и журнал ошибок |

Статистика хранится в памяти и обнуляется при перезапуске.
## GET /jsondb/save

Немедленно сохраняет `masterDb` (индекс FileDB) в `Data/masterDb.bz` в фоне. Кроме этого, изменённый `masterDb` сохраняется автоматически примерно раз в 10 минут. Ручной вызов нужен перед остановкой или обновлением. В `Data/crontab` он стоит каждые 5 минут.

| Ответ | Значение |
| --- | --- |
| `ok` | Сохранение запущено |
| `work` | Предыдущее сохранение ещё идёт |
| `syncapi` | Экземпляр работает как клиент `syncapi`, и ручное сохранение отключено |

```bash
curl "http://127.0.0.1:9117/jsondb/save"
```

## Проверка FileDB

### GET /cron/maintenance/Check

Запускает в фоне проверку целостности FileDB (не дольше 6 часов). Отвечает сразу: `ok`, если проверка запущена, или `work`, если она уже идёт. Во время работы задача видна в [`/health/background-jobs`](health.md#get-healthbackground-jobs) как `maintenance:check`.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `mode` | string | `report` | `report` - только отчёт, `safe` - отчёт и безопасные исправления, `full` - все исправления. Неизвестное значение считается `report` |
| `sampleSize` | integer | `20` | Сколько примеров сохранять по каждой проблеме (1..200; меньше 1 превращается в 20) |
| `excludeNumericXx` | boolean | `true` | Не считать ошибкой числовые ключи вида `1899:1899` |

Что делает каждый режим:

| Режим | Действия |
| --- | --- |
| `report` | Только читает базу. Отчёт пишется в `Data/temp/maintenance-last.json` |
| `safe` | Заполняет пустые `_sn`/`_so`/`name`/`originalname`, удаляет `null`-записи, переносит записи, у которых изменился ключ бакета, убирает пустые бакеты из `masterDb`. Затем сохраняет `masterDb` и перестраивает быстрый индекс |
| `full` | Всё из `safe`, а также: удаляет записи без магнита и без типов, исправляет ключи словаря, не совпадающие с `url`, удаляет из `masterDb` ключи без файла шарда и удаляет файлы шардов, которых нет в `masterDb` |

```bash
curl "http://127.0.0.1:9117/cron/maintenance/Check?mode=report"
curl "http://127.0.0.1:9117/cron/maintenance/Check?mode=safe&sampleSize=50"
```

:::warning[Внимание]
Режим `full` удаляет данные. Перед запуском сделайте резервную копию `Data/fdb` и `Data/masterDb.bz`. Ту же проверку можно запустить без HTTP командой `crabindex maintain --mode=report|safe|full`, подробнее в разделе [Обслуживание FileDB](../operations/maintenance.md).
:::

### GET /cron/maintenance/Status

Состояние текущей проверки и последний отчёт. Если данных нет, поле в ответе отсутствует.

```bash
curl "http://127.0.0.1:9117/cron/maintenance/Status"
```

```json
{
  "ok": true,
  "running": true,
  "mode": "safe",
  "startedAt": "2026-09-25T05:00:00.123Z",
  "progress": { "current": 51234, "total": 390112, "detail": "fix" },
  "last": {
    "ok": true,
    "mode": "report",
    "running": false,
    "startedAt": "2026-09-21T05:00:00.101Z",
    "finishedAt": "2026-09-21T05:06:12.884Z",
    "durationSec": 372.8,
    "totals": { "fdbKeys": 390112, "torrents": 3110540 },
    "issues": {
      "nullValue": { "count": 0, "sample": [] },
      "missingName": { "count": 2, "sample": [] },
      "missingOriginalname": { "count": 2, "sample": [] },
      "missingTrackerName": { "count": 0, "sample": [] },
      "emptySearchFields": { "emptySn": { "count": 0, "sample": [] }, "emptySo": { "count": 0, "sample": [] }, "emptyBoth": { "count": 0, "sample": [] }, "total": 0 },
      "xxKeys": { "count": 5, "sample": [] },
      "bucketMismatch": { "count": 14, "sample": [] },
      "urlKeyMismatch": { "count": 0, "sample": [] },
      "missingShardFile": { "count": 0, "sample": [] },
      "emptyShardListed": { "count": 1, "sample": [] },
      "orphanShardFiles": { "count": 0, "sample": [] },
      "emptyMagnetOrTypes": { "count": 37, "sample": [] }
    },
    "fixed": { "nullRemoved": 0, "searchFieldsFixed": 0, "migrated": 0, "emptyBucketsRemoved": 0, "missingShardKeysRemoved": 0, "orphansDeleted": 0, "urlKeyFixed": 0, "incompleteRemoved": 0 }
  }
}
```

Во время работы `progress.detail` принимает значения: `fix` (safe-исправления), `full-fix`, `save`.

## Циклы ParseAll

### GET /cron/maintenance/ParseAllStatus

Для каждого трекера с `ParseAllTask` показывает, сколько страниц текущего цикла ещё не пройдено (по файлам `Data/temp/{tracker}_parseAllCycle.json` и `{tracker}_taskParse.json`) и идёт ли задача сейчас.

```bash
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
```

```json
[
  {
    "tracker": "kinozal",
    "running": false,
    "cycleId": "8c1f0d3b6a0e4b4c9a51f1e0f5d2c7aa",
    "pending": 0,
    "mapCount": 3920,
    "cycleStartedAtUtc": "2026-09-25T02:35:00.412Z"
  },
  {
    "tracker": "rutracker",
    "running": true,
    "cycleId": "0b8e6f9a2c9d4d7fa3e1a2b3c4d5e6f7",
    "pending": 11234,
    "mapCount": 16020,
    "cycleStartedAtUtc": "2026-09-23T01:40:00.000Z",
    "pagesCompleted": 812,
    "pagesTotal": 12046
  }
]
```

| Поле | Описание |
| --- | --- |
| `tracker` | Slug трекера |
| `running` | `ParseAllTask` сейчас выполняется |
| `cycleId` | Идентификатор текущего цикла (нет, если цикл ещё не создавался) |
| `pending` | Страниц карты, не пройденных в этом цикле |
| `mapCount` | Всего страниц в карте |
| `cycleStartedAtUtc` | Начало цикла |
| `pagesCompleted`, `pagesTotal` | Прогресс текущего запуска (только при `running: true`) |

### GET /cron/maintenance/ResumeParseAll

Продолжает незавершённые циклы, например после перезапуска или обновления. Для каждого трекера результат такой:

- `work` - `ParseAllTask` уже идёт;
- `idle` - цикла нет или `pending = 0`. Новый круг здесь **не** начинается, его запускает очередной вызов `/cron/{tracker}/ParseAllTask`;
- `ok` - `ParseAllTask` запущен и продолжит цикл с места остановки;
- `disabled` - трекер в `disable_trackers` (для трекеров, которые это проверяют).

```bash
curl "http://127.0.0.1:9117/cron/maintenance/ResumeParseAll"
```

```json
{
  "started": 1,
  "jobs": [
    { "tracker": "kinozal", "running": false, "result": "idle", "cycleId": "8c1f...", "pending": 0, "mapCount": 3920, "cycleStartedAtUtc": "2026-09-25T02:35:00.412Z" },
    { "tracker": "rutracker", "running": false, "result": "ok", "cycleId": "0b8e...", "pending": 11234, "mapCount": 16020, "cycleStartedAtUtc": "2026-09-23T01:40:00.000Z" }
  ]
}
```

CrabIndex вызывает ту же процедуру сам через 45 секунд после старта. В `Data/crontab` она дополнительно стоит каждые 15 минут как страховка.

## Мониторинг

- [`GET /health/background-jobs`](health.md#get-healthbackground-jobs) - фоновые задачи с прогрессом;
- `Data/log/{tracker}.log` - подробный лог парсера (при `logParsers: true` и `log: true` в блоке трекера);
- строки `cron:` в консоли (`journalctl -u crabindex` или `docker logs`).

:::note[Примечание]
Не запускайте одну и ту же задачу параллельно из нескольких планировщиков. `Data/run-job.sh` защищает от наложения запусков через `flock` и ограничивает время `curl --max-time`.
:::

## См. также

- [Cron: установка расписания](../deployment/cron.md)
- [Фоновые процессы](../concepts/background-jobs.md)
- [Обзор трекеров](../trackers/overview.mdx)
- [Обслуживание FileDB](../operations/maintenance.md)
