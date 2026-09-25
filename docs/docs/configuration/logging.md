# Логирование

CrabIndex пишет логи в два места:

- **консоль** (stdout/stderr) - её собирает `journalctl` при запуске через systemd или `docker logs` в контейнере. Сообщения уровня Warning и выше идут в stderr, остальные - в stdout;
- **файлы** в `Data/log/` - подробные журналы FileDB, парсеров и модуля Tracks.

## Файлы в Data/log

| Файл | Что пишется | Управляется |
| --- | --- | --- |
| `fdb.ГГГГ-ММ-ДД.log` | Каждое добавление и изменение раздачи в FileDB (JSON-строка «было / стало») | `logFdb`, `logFdbRetentionDays`, `logFdbMaxSizeMb`, `logFdbMaxFiles` |
| `{трекер}.log` | Ход работы парсера: страницы, категории, ошибки, итоги | `logParsers` и `log` в блоке трекера |
| `tracks.log` | Анализ дорожек через TorrServer | `trackslog` |

```yaml
logFdb: true
logFdbRetentionDays: 7    # удалять файлы старше N дней; 0 - не удалять
logFdbMaxSizeMb: 0        # общий лимит размера fdb-журналов, МБ; 0 - без лимита
logFdbMaxFiles: 0         # лимит числа fdb-журналов; 0 - без лимита
logParsers: true          # журналы парсеров Data/log/{трекер}.log
trackslog: true           # Data/log/tracks.log
```

Журнал парсера пишется, только если включены и `logParsers`, и `log` в блоке конкретного трекера:

```yaml
Rutor:
  log: false              # не писать Data/log/rutor.log
```

:::tip[Совет]
На маленьких дисках выключите журнал FileDB (`logFdb: false`) или ограничьте его (`logFdbMaxSizeMb: 500`, `logFdbMaxFiles: 5`): при первой синхронизации он растёт очень быстро. Журналы парсеров и `tracks.log` не ротируются автоматически - используйте `logrotate` или периодически очищайте их.
:::

## Консоль: блок logging

```yaml
logging:
  defaultLevel: Information
  consoleTimestamp: false
  tracksConsoleDetail: false
  cronSkipFastMs: 100
  categories:
    parsers: None
    fdb: Warning
    sync_spidr: Warning
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `defaultLevel` | `Information` | Минимальный уровень для категорий без явной настройки: `Trace`, `Debug`, `Information`, `Warning`, `Error`, `Critical`, `None` |
| `consoleTimestamp` | `false` | Добавлять время `[ЧЧ:ММ:СС]` в строку (journald и Docker добавляют своё) |
| `tracksConsoleDetail` | `false` | Печатать в консоль все шаги модуля Tracks |
| `cronSkipFastMs` | `100` | Успешные запросы `/cron/*` быстрее N мс логируются на уровне Debug (то есть скрыты при `Information`). `0` - логировать все |
| `categories` | `parsers: None` | Уровень для отдельных категорий. `None` полностью отключает категорию |

Каждая строка консоли начинается с имени категории: `sync: …`, `fdb: …`, `cron: [12:00:01] rutor/parse 1.5s 200`.

### Категории

| Категория | Что пишет |
| --- | --- |
| `host` | Запуск и остановка сервера, Cloudflare/FlareSolverr, сетевые ошибки |
| `config` | Загрузка и перезагрузка конфигурации (секреты замаскированы) |
| `cron` | Запросы `/cron/*`: путь, длительность, код ответа (`FAIL` для ошибок) |
| `fdb` | FileDB: сохранение `masterDb`, вытеснение кеша, обслуживание |
| `fastdb` | Перестройка поискового индекса |
| `sync` | Загрузка раздач с `syncapi` |
| `sync_spidr` | Обновление сидов/пиров с `syncapi` (можно писать как `syncSpidr`) |
| `stats` | Сбор статистики |
| `trackers` | Фоновые задачи трекеров, сборка `trackers.txt` |
| `parsers` | Подробности парсеров в консоли. По умолчанию `None`: подробности идут только в `Data/log/{трекер}.log` |
| `tracks`, `tracks index`, `tracks stats`, `tracks export` | Модуль Tracks |
| `security` | Самопроверка таблицы доступа при старте |

## Примеры

### Тихий режим

```yaml
logFdb: false
logging:
  defaultLevel: Warning
  categories:
    cron: Information
    sync_spidr: None
```

### Отладка парсера

```yaml
logParsers: true
logging:
  cronSkipFastMs: 0
  categories:
    parsers: Debug
    cron: Debug
```

```bash
tail -f Data/log/rutracker.log
```

Изменения блока `logging` применяются вместе с горячей перезагрузкой конфигурации, перезапуск не нужен.

## Где смотреть логи

```bash
journalctl -u crabindex -f              # systemd
docker logs -f crabindex                # Docker
tail -f /opt/crabindex/Data/log/kinozal.log
```
