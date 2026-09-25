# Статистика

Stats API отдаёт сводку по трекерам в FileDB, статистику модуля Tracks и метки времени последнего пересчёта. Эти данные показывает страница `/stats` веб-интерфейса.

## Доступ

- Если задан `apikey`, маршруты требуют ключ, как и поиск (см. [Матрица доступа](../operations/access-matrix.md)).
- Флаг `openstats` (по умолчанию `true`) управляет содержимым ответа. При `openstats: false` маршруты отвечают `200`, но без данных: `/stats/torrents` возвращает `[]`, `/stats/tracks` и `/stats/meta` возвращают `{"ok": false}`.

```yaml
openstats: true
timeStatsUpdate: 90 # минут между пересчётами
```

## Как считается статистика

Фоновый процесс впервые проходит по FileDB через 20 секунд после старта, а затем повторяет проход каждые `timeStatsUpdate` минут (по умолчанию 90, в `Data/example.yaml` стоит 15). При `timeStatsUpdate: -1` периодический пересчёт приостанавливается. Это значение можно задать только правкой файла: [Config API](config.md) требует `timeStatsUpdate` ≥ 1. За один проход обновляются три файла:

| Файл | Содержимое |
| --- | --- |
| `Data/temp/stats.json` | Строки по трекерам, которые отдаёт `/stats/torrents` |
| `Data/temp/stats-meta.json` | `updatedAt`, `updatedAtLocal`, `trackerCount` |
| `Data/temp/tracks-stats.json` (кеш модуля Tracks) | Сводка, которую отдаёт `/stats/tracks` |

## GET /stats/torrents

Отдаёт содержимое `Data/temp/stats.json`: массив трекеров, отсортированный по `alltorrents` по убыванию.

```bash
curl "http://127.0.0.1:9117/stats/torrents?apikey=YOUR_API_KEY"
```

```json
[
  {
    "trackerName": "rutracker",
    "lastnewtor": "25.09.2026",
    "newtor": 812,
    "update": 10433,
    "check": 10433,
    "alltorrents": 1498211,
    "tracks": { "wait": 1302110, "confirm": 190455, "skip": 5646 }
  },
  {
    "trackerName": "kinozal",
    "lastnewtor": "25.09.2026",
    "newtor": 240,
    "update": 3120,
    "check": 3120,
    "alltorrents": 551870,
    "tracks": { "wait": 470002, "confirm": 80911, "skip": 957 }
  }
]
```

| Поле | Описание |
| --- | --- |
| `trackerName` | Slug трекера |
| `lastnewtor` | Дата самой свежей `createTime` у раздач трекера, `дд.ММ.гггг` |
| `newtor` | Сколько раздач создано сегодня (UTC) |
| `update` | Сколько раздач обновлено сегодня (`updateTime`) |
| `check` | Сколько раздач проверено сегодня (`checkTime`) |
| `alltorrents` | Всего раздач трекера в FileDB |
| `tracks.confirm` | Раздачи с магнитом, для которых уже есть аудио/видео-дорожки |
| `tracks.wait` | Раздачи с магнитом, которые ждут анализа |
| `tracks.skip` | Раздачи, исключённые из анализа после `tracksatempt` неудачных попыток |

Раздачи без магнита и раздачи «неподходящих» типов в блок `tracks` не входят.

## GET /stats/tracks

Сводка модуля [Tracks](../concepts/tracks.md): сколько записей о дорожках найдено и откуда. Обычно ответ берётся из кеша. Если кеша нет, выполняется полный проход, который может занять заметное время.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `includeTorrentDb` | boolean | `true` | Учитывать поля `ffprobe` раздач в FileDB в дополнение к файлам `Data/tracks` |

```bash
curl "http://127.0.0.1:9117/stats/tracks?includeTorrentDb=true&apikey=YOUR_API_KEY"
```

```json
{
  "ok": true,
  "updatedAt": "2026-09-25T12:00:03.1234567Z",
  "fromCache": true,
  "stats": {
    "total": 190455,
    "filesScanned": 184002,
    "fromTracksFiles": 184002,
    "fromMemory": 0,
    "fromTorrentDb": 6453,
    "torrentsScanned": 3110540,
    "invalidPath": 0,
    "emptyStreams": 12,
    "readErrors": 0,
    "magnetErrors": 3,
    "torrentDbErrors": 0
  }
}
```

| Поле | Описание |
| --- | --- |
| `updatedAt` | Когда был построен кеш, UTC |
| `fromCache` | `true`, если ответ взят из кеша без нового прохода |
| `stats.total` | Уникальных раздач с дорожками |
| `stats.filesScanned` | Прочитано файлов в `Data/tracks` |
| `stats.fromTracksFiles` / `fromMemory` / `fromTorrentDb` | Откуда взяты записи |
| `stats.torrentsScanned` | Просмотрено раздач FileDB (при `includeTorrentDb=true`) |
| `stats.invalidPath`, `emptyStreams`, `readErrors`, `magnetErrors`, `torrentDbErrors` | Счётчики проблем |

Принудительно пересчитать эту статистику можно через [`/dev/TracksStats?refresh=true`](dev.md#tracks).

## GET /stats/meta

Метки времени последнего пересчёта. Удобно проверять, работает ли фоновый сбор статистики.

```bash
curl "http://127.0.0.1:9117/stats/meta?apikey=YOUR_API_KEY"
```

```json
{
  "ok": true,
  "updatedAt": "2026-09-25T12:00:03.1234567Z",
  "updatedAtLocal": "2026-09-25 15:00:03",
  "tracksStatsUpdatedAt": "2026-09-25T12:00:03.1234567Z"
}
```

| Поле | Описание |
| --- | --- |
| `updatedAt` | Время последнего прохода по FileDB, UTC |
| `updatedAtLocal` | То же время в часовом поясе сервера |
| `tracksStatsUpdatedAt` | Время построения кеша статистики Tracks |

Если данных ещё нет (например, сразу после первого старта), поля с датами отсутствуют.

## См. также

- [Конфигурация: обзор](../configuration/overview.md): `openstats`, `timeStatsUpdate`
- [Tracks](../concepts/tracks.md)
- [Веб-интерфейс](../concepts/web-ui.md)
