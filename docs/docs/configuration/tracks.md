# Tracks

Параметры модуля [Tracks](../concepts/tracks.md): анализ аудиодорожек раздач через TorrServer и `ffprobe`.

## Включение

```yaml
tracks: true
trackslog: true
trackscategory: crabindex-home
tsuri:
  - http://127.0.0.1:8090
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `tracks` | `false` | Включить анализ |
| `trackslog` | `true` | Писать журнал в `Data/log/tracks.log` |
| `trackscategory` | `crabindex` | Категория, в которую модуль добавляет торренты в TorrServer. По ней же удаляются «осиротевшие» торренты |
| `tsuri` | `["http://127.0.0.1:8090"]` | Список серверов TorrServer. Можно указать учётные данные в URL: `http://user:pass@host:8090` |

:::warning[Внимание]
`trackscategory` должна быть уникальной для каждого инстанса, который использует общий TorrServer. Иначе инстансы будут удалять торренты друг друга при очистке «сирот».
:::

## Очередь и таймауты

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `tracksdelay` | `20000` | Пауза между раздачами, мс (±10%) |
| `tracksconcurrency` | `2` | Сколько анализов выполняется одновременно на все серверы |
| `tracksatempt` | `20` | После стольких неудач раздача исключается из анализа |
| `tracksffptimeout` | `60` | Таймаут `/ffp`, если у раздачи есть сиды, секунды |
| `tracksffptimeoutnosid` | `30` | Таймаут `/ffp`, если сидов нет, секунды |
| `tracksreadtimeout` | `30` | Ожидание статистики файлов от TorrServer, секунды |
| `trackspeerwaittimeout` | `30` | Ожидание сидов и первых байтов перед `/ffp`, секунды |
| `tracksffpretry` | `2` | Сколько дополнительных файлов раздачи пробовать за одну попытку |
| `tracksminbufferkb` | `512` | Минимальный буфер перед вызовом `/ffp`, КБ |
| `tracksorphansweepmin` | `15` | Интервал очистки «осиротевших» торрентов в `trackscategory`, минуты |

## Режим и расписание

```yaml
tracksmod: 0
tracksinterval:
  task1: 60
  task0: 180
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `tracksmod` | `0` | `0` - анализировать раздачи любого возраста; `1` - не выполнять задачи для раздач старше месяца (typetask 3 и 4) |
| `tracksinterval.task1` | `60` | Пауза между проходами по раздачам за последние сутки, минуты |
| `tracksinterval.task0` | `180` | Базовая пауза для остальных задач, минуты (к ней добавляется номер задачи) |

## Несколько серверов TorrServer

```yaml
tsuri:
  - http://ts-1:8090
  - http://ts-2:8090
  - http://ts-3:8090
tracksconcurrency: 6
```

Для каждой раздачи выбирается доступный сервер с учётом текущей загрузки. Общее число одновременных анализов ограничивает `tracksconcurrency`.

## Логи консоли

Подробные шаги модуля в консоли включает `logging.tracksConsoleDetail: true`; уровень категории задаётся в `logging.categories.tracks`. См. [Логирование](logging.md).

## Мониторинг

```bash
curl "http://localhost:9117/stats/tracks"
curl "http://localhost:9117/dev/TracksStats"      # локальная сеть или devkey
tail -f Data/log/tracks.log
```

## Отключение

```yaml
tracks: false
```

Уже собранные данные остаются в `Data/tracks/` и продолжают использоваться в ответах поиска.
