# Обслуживание FileDB

CrabIndex умеет проверять целостность FileDB - ключи `masterDb`, файлы шардов и сами записи раздач - и исправлять найденное. Начинайте всегда с режима `report`, затем выбирайте исправляющий режим по отчёту.

Проверку, сохранение базы, диагностику и миграции удобно запускать из раздела **Обслуживание** [админ-панели](../admin.md): там же видно состояние текущей проверки. Ниже описаны те же операции через HTTP и командную строку.

## Режимы

| Режим | Что делает |
| --- | --- |
| `report` | Только проверяет и пишет отчёт. Ничего не меняет |
| `safe` | `report` + заполняет пустые поисковые поля и названия, удаляет `null`-записи, переносит записи, чей ключ бакета изменился, убирает пустые бакеты из `masterDb`, затем сохраняет `masterDb` и перестраивает поисковый индекс |
| `full` | `safe` + удаляет записи без magnet и без категорий, исправляет записи, у которых ключ не совпадает с URL, повторно переносит несовпадающие бакеты, удаляет из `masterDb` ключи без файла шарда и удаляет «осиротевшие» файлы шардов |

Что проверяется: `null`-записи, пустые `name` / `originalname` / трекер, пустые поисковые поля, несовпадение бакета и ключа, несовпадение URL и ключа записи, записи без magnet или категорий, ключи без файла, пустые шарды, ключи вида `название:название`, файлы шардов без ключа.

Отчёт сохраняется в `Data/temp/maintenance-last.json`. Для каждого типа проблем в нём есть счётчик и до `sampleSize` примеров (1-200, по умолчанию 20).

## Проверка на работающем сервере

`Check` запускает проверку в фоне (лимит - 6 часов) и сразу отвечает `ok` или `work` (проверка уже идёт). `Status` показывает прогресс и последний отчёт.

```bash
curl "http://127.0.0.1:9117/cron/maintenance/Check?mode=report&sampleSize=20"
curl "http://127.0.0.1:9117/cron/maintenance/Status"
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `mode` | `report` | `report`, `safe` или `full` |
| `sampleSize` | `20` | Примеров на тип проблемы, 1-200 |
| `excludeNumericXx` | `true` | Не считать проблемой числовые ключи вида `1899:1899` |

Пути `/cron/*` - политика DevAdmin: через прокси или туннель нужен `devkey`. См. [Матрица доступа](access-matrix.md).

Стандартный `Data/crontab` запускает проверку в режиме `report` каждое воскресенье в 05:00:

```text
0 5 * * 0  /opt/crabindex/Data/run-job.sh maintenance-check http://127.0.0.1:9117/cron/maintenance/Check?mode=report 60
```

## Автономная проверка: crabindex maintain

Та же проверка без HTTP-сервера и без лимита в 6 часов:

```text
crabindex maintain [--mode=report|safe|full] [--sample-size=20] [--include-numeric-xx]
```

| Опция | Описание |
| --- | --- |
| `--mode=` | `report` (по умолчанию), `safe`, `full` |
| `--sample-size=` | Примеров на тип проблемы, положительное число |
| `--include-numeric-xx` | Учитывать и числовые ключи вида `1899:1899` |
| `--help`, `-h` | Справка |

Запускайте из каталога установки (где лежат `Data/` и `init.yaml` - от него зависит раскладка `fdbPathLevels`).

### Порядок действий

```bash
cd /opt/crabindex

# 1. Отчёт (можно при работающем сервере)
sudo -u crabindex ./crabindex maintain --mode=report

# 2. Остановите сервер перед исправлением
sudo systemctl stop crabindex

# 3. Резервная копия и исправление
sudo tar -czf ~/fdb-backup.tgz Data/fdb Data/masterDb.bz
sudo -u crabindex ./crabindex maintain --mode=safe --sample-size=50

# 4. Запустите сервер
sudo systemctl start crabindex
```

В Docker:

```bash
docker compose stop crabindex
docker compose run --rm crabindex ./crabindex maintain --mode=report
docker compose start crabindex
```

Коды завершения:

| Код | Значение |
| --- | --- |
| `0` | Успешно (или выведена справка) |
| `1` | Проверка завершилась с ошибками |
| `2` | Неверные аргументы |
| `130` | Отменено по Ctrl+C |

Ctrl+C корректно прерывает проход.

:::warning[Внимание]
Не запускайте `safe` и `full` параллельно с работающим сервером: оба процесса будут менять один и тот же `Data/`. Перед `full` обязательно сделайте резервную копию - этот режим удаляет записи и файлы.
:::

## Другие операции

- `GET /jsondb/save` - немедленно сохранить `masterDb` (см. [FileDB](../concepts/filedb.md)).
- `GET /cron/maintenance/ParseAllStatus` - состояние циклов ParseAll на диске.
- `GET /cron/maintenance/ResumeParseAll` - продолжить незавершённые циклы ParseAll.
- `/dev/*` - точечные диагностики и миграции (`FindCorrupt`, `FindDuplicateKeys`, `RemoveNullValues`, `UpdateSize` и др.), см. [Dev API](../api-reference/dev.md).
