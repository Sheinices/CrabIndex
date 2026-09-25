# Dev

Маршруты `/dev/*` предназначены для администрирования: диагностика FileDB, массовые пересчёты, разовые миграции данных и обслуживание модуля Tracks. Клиентские приложения на них не опираются.

## Доступ

Как и `/cron/*`, маршруты `/dev/*` доступны из LAN и с localhost напрямую, а извне и через обратный прокси только с `devkey` (`X-Dev-Key` или `?devkey=`). Подробнее в [Матрице доступа](../operations/access-matrix.md).

Диагностику и миграции можно запускать и из раздела **Обслуживание** [админ-панели](../admin.md): панель вызывает те же маршруты как `{admin.path}/api/dev/*`.

Все маршруты принимают любой HTTP-метод и выполняются **синхронно**: ответ приходит, когда операция закончена. На большой базе это может занять много минут, поэтому увеличьте таймаут клиента (`curl --max-time`). JSON-ответы не содержат полей со значением `null`.

```bash
curl --max-time 3600 -H "X-Dev-Key: YOUR_DEV_KEY" \
  "https://crabindex.example.com/dev/FindCorrupt?sampleSize=50"
```

:::warning[Внимание]
Маршруты пересчёта и миграции переписывают FileDB. Перед запуском сделайте резервную копию `Data/fdb` и `Data/masterDb.bz` и не запускайте их параллельно с `/cron/maintenance/Check?mode=full`.
:::

## Диагностика (только чтение)

### GET /dev/FindCorrupt

Ищет записи, сохранённые как `null`, и записи без `name`, `originalname` или `trackerName`.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `sampleSize` | integer | `20` | Сколько примеров вернуть по каждой проблеме |

```json
{
  "ok": true,
  "totalFdbKeys": 390112,
  "totalTorrents": 3110540,
  "corrupt": {
    "nullValue": { "count": 0, "sample": [] },
    "missingName": { "count": 1, "sample": [{ "fdbKey": "...", "url": "...", "title": "..." }] },
    "missingOriginalname": { "count": 1, "sample": [] },
    "missingTrackerName": { "count": 0, "sample": [] }
  }
}
```

### GET /dev/FindDuplicateKeys

Ищет бакеты с ключом вида `имя:имя`, где обе части совпадают без учёта регистра.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `tracker` | string | - | Оставить только бакеты, в которых есть раздачи этого трекера |
| `excludeNumeric` | boolean | `true` | Пропускать числовые ключи (`1899:1899`) |

```json
{ "ok": true, "count": 2, "keys": [{ "key": "ponies:ponies", "count": 3 }] }
```

### GET /dev/FindEmptySearchFields

Ищет записи с пустыми поисковыми полями `_sn` и/или `_so`. Такие записи не находятся поиском.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `sampleSize` | integer | `20` | Сколько примеров вернуть |

Ответ: `{ ok, totalFdbKeys, totalTorrents, emptySearchFields: { emptySn, emptySo, emptyBoth, total } }`.

Полный отчёт по всем проверкам сразу строит [`/cron/maintenance/Check`](cron.md#get-cronmaintenancecheck).

## Массовые пересчёты

Эти маршруты переписывают каждую запись FileDB и отвечают `{"ok": true}`.

| Маршрут | Что делает |
| --- | --- |
| `GET /dev/UpdateSize` | Пересчитывает `size` в байтах из `sizeName` (`700 MB`, `1,5 ГБ`, `2 TB`...) и обновляет `updateTime` |
| `GET /dev/ResetCheckTime` | Ставит всем записям `checkTime` на вчерашний день |
| `GET /dev/UpdateDetails` | Пересчитывает производные поля по заголовку (`size`, `quality`, `videotype` SDR/HDR, озвучки `voices` и т. д.), очищает `languages`, обновляет `updateTime` |
| `GET /dev/UpdateSearchName` | Заполняет пустые `name`/`originalname` из `title`, пересобирает `_sn`/`_so` и переносит записи, у которых изменился ключ бакета |

:::note[Примечание]
`UpdateSize` и `UpdateDetails` меняют `updateTime`, поэтому после них клиенты синхронизации (`/sync/fdb/torrents`) заново скачают всю базу.
:::

## Миграции и чистка

Разовые исправления после изменений в парсерах. Повторный запуск безопасен, но обычно не нужен.

| Маршрут | Что делает | Ответ |
| --- | --- | --- |
| `GET /dev/RemoveNullValues` | Удаляет записи, сохранённые как `null` | `{ ok, removed, affectedFiles }` |
| `GET /dev/FixEmptySearchFields` | Заполняет пустые `_sn`/`_so`, переносит записи в правильные бакеты, перестраивает быстрый индекс | `{ ok, totalFixed, snFixed, soFixed, migrated, affectedBuckets }` |
| `GET /dev/RemoveBucket` | Удаляет бакет или переносит все его записи под новое имя (см. ниже) | `{ ok, key, migrated, removed, newKey }` |
| `GET /dev/FixKnabenNames` | Knaben: заново выделяет название, год и заголовок из сохранённого `title` | `{ ok, processed, updated, migrated }` |
| `GET /dev/FixBitruNames` | BitRu: убирает из `name`/`originalname` номера сезонов и пометки качества | `{ ok, processed, updated, migrated }` |
| `GET /dev/FixRudubRelased` | RuDub: заполняет год (`relased`) и обрезанные названия по сохранённому заголовку | `{ ok, processed, yearUpdated, namesUpdated, migrated }` |
| `GET /dev/MigrateAnilibertyUrls` | AniLiberty: добавляет `hash=<btih>` к URL, где его нет | `{ ok, totalProcessed, totalUpdated, totalSkipped, totalErrors, errors }` |
| `GET /dev/RemoveDuplicateAniliberty` | AniLiberty: для каждого infohash оставляет самую свежую запись | `{ ok, totalProcessed, totalRemoved, duplicatesFound, duplicates }` |
| `GET /dev/FixAnimelayerDuplicates` | AnimeLayer: сливает дубликаты раздач | `{ ok, totalProcessed, totalFixed, totalRemoved, totalErrors, errors }` |
| `GET /dev/FixKinozalDomainDuplicates` | Kinozal: сводит URL к домену из `Kinozal.host`, сливает дубли одной раздачи с разных доменов, удаляет ссылки на `userdetails.php` | `{ ok, scanned, rewritten, merged, removed, canonicalHost }` |
| `GET /dev/FixUltradoxDomainDuplicates` | Ultradox: переписывает URL на домен из `Ultradox.host` и сливает дубли по пути и фрагменту | `{ ok, scanned, rewritten, merged, removed, canonicalHost }` |

При слиянии дублей сохраняются лучшие `sid`/`pir`, самый свежий `updateTime` и магнит, если у оставляемой записи его не было.

### GET /dev/RemoveBucket

| Параметр | Тип | Описание |
| --- | --- | --- |
| `key` | string | Ключ бакета `name:originalname`, обязателен |
| `migrateName` | string | Новое `name` для записей бакета |
| `migrateOriginalname` | string | Новое `originalname` |

Если переданы оба `migrate*`, записи переносятся в бакет `migrateName:migrateOriginalname`. Если нет, бакет удаляется целиком.

```bash
# Удалить бакет
curl "http://127.0.0.1:9117/dev/RemoveBucket?key=ponies:ponies"

# Перенести записи под правильное имя
curl "http://127.0.0.1:9117/dev/RemoveBucket?key=ponies:ponies&migrateName=пони&migrateOriginalname=ponies"
```

Ошибки возвращаются в поле `error`: `key required, format: name:originalname (e.g. ponies:ponies)` или `key not found`.

## Tracks

Администрирование модуля [Tracks](../concepts/tracks.md): аудио- и видеодорожки, полученные через TorrServer (`tsuri`). Булевы параметры принимают `true`/`false` без учёта регистра.

### GET /dev/TracksStats

Та же статистика, что [`/stats/tracks`](stats.md#get-statstracks), но доступна без `openstats` и умеет пересчитывать данные принудительно.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `includeTorrentDb` | boolean | `true` | Учитывать поля `ffprobe` в FileDB |
| `refresh` | boolean | `false` | Игнорировать кеш и выполнить полный проход |

### GET /dev/ExportTracks

Экспортирует все известные дорожки в JSON-файлы `{aa}/{b}/{hash}.json` внутри выбранного каталога.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `dir` | string | `Data/tracks-export` | Каталог назначения. Должен лежать внутри `Data/`, иначе ответ `500` |
| `dryRun` | boolean | `false` | Только посчитать, ничего не записывать |
| `includeTorrentDb` | boolean | `true` | Добавлять дорожки из полей `ffprobe` FileDB |
| `background` | boolean | `true` | Запустить в фоне и сразу ответить |

Ответы:

- `{"ok": true, "started": true, "status": {...}}` - экспорт запущен в фоне;
- `{"ok": false, "alreadyRunning": true, "status": {...}}` - экспорт уже идёт;
- `{"ok": true, "result": {...}}` - при `dryRun=true` или `background=false`; `result` содержит `outputDir`, `dryRun`, `includeTorrentDb`, `stats`, `written`, `writeErrors`, `errorSamples`.

### GET /dev/ExportTracksStatus

Состояние фонового экспорта: `{ ok: true, status: { running, phase, outputDir, includeTorrentDb, startedAt, completedAt, total, written, writeErrors, stats, result, error } }`.

### GET /dev/BackfillTracks

Дополняет каталог `Data/tracks`: переносит файлы старого формата и создаёт файлы для раздач, у которых дорожки есть только в FileDB.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `dryRun` | boolean | `false` | Только посчитать |
| `migrateLegacy` | boolean | `true` | Переносить файлы старого формата |
| `includeTorrentDb` | boolean | `true` | Брать дорожки из полей `ffprobe` FileDB |

Ответ: `{ ok: true, result: { tracksDir, dryRun, includeTorrentDb, migrateLegacy, stats, written, migratedLegacy, skippedExisting, writeErrors, errorSamples } }`.

```bash
curl "http://127.0.0.1:9117/dev/TracksStats?refresh=true"
curl "http://127.0.0.1:9117/dev/ExportTracks?dir=Data/tracks-export&background=true"
curl "http://127.0.0.1:9117/dev/ExportTracksStatus"
curl "http://127.0.0.1:9117/dev/BackfillTracks?dryRun=true"
```

## См. также

- [Обслуживание FileDB](../operations/maintenance.md)
- [Cron: проверка FileDB](cron.md#проверка-filedb)
- [FileDB](../concepts/filedb.md)
