# Toloka

![](/img/trackers/toloka.ico)

Toloka (slug `toloka`) - украинский торрент-трекер (toloka.to), около 58 тыс. раздач с украинской озвучкой: фильмы, сериалы, мультфильмы, HD-разделы, документальное и ТВ-шоу. Для скачивания `.torrent` нужен аккаунт.

Toloka использует полный набор задач: частый `parse` первых страниц, ежедневный пересчёт карты страниц `UpdateTasksParse` и возобновляемый полный обход `ParseAllTask`. Для новых раздач и раздач с изменённым заголовком скачивается `.torrent` (`download.php?id=<id>`), из которого строится магнет-ссылка.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Toloka` |
| `host` | `https://toloka.to` |
| `alias` | не используется |
| `reqMinute` | `8` → `parseDelay` 7000 мс (пауза между страницами `ParseAllTask` / `ParseLatest`) |
| `useproxy` | не используется |
| Авторизация | обязательна: `login.u` / `login.p` (статический `cookie` не поддерживается) |
| Кодировка | UTF-8 |
| Магнет-ссылки | из скачанного `.torrent` |
| Учитывает `disable_trackers` | нет |

## Конфигурация

```yaml
Toloka:
  host: https://toloka.to
  reqMinute: 8
  log: true
  login:
    u: "your_login"
    p: "your_password"
```

CrabIndex отправляет форму `login.php`, сохраняет cookie `toloka_sid` и `toloka_data` в памяти на 1 час и затем логинится снова. После неудачного входа следующая попытка - не раньше чем через 5 минут; в это время страницы считаются неуспешными.

## Cron-маршруты

Маршруты принимают `GET` и `POST`, пути и имена параметров регистронезависимы.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/toloka/parse` | `page` (0) | лог `<раздел> - <страница>` по строке на раздел; `work`, если parse уже идёт | Синхронно разбирает страницу `page` восьми основных разделов (16, 96, 19, 139, 32, 173, 174, 44) |
| `/cron/toloka/UpdateTasksParse` | нет | `ok` / `work` сразу; `TakeLogin == null`, если войти не удалось | В фоне перечитывает пейджер 22 разделов и перестраивает карту страниц. Лимит - 30 минут |
| `/cron/toloka/ParseAllTask` | нет | `ok` / `work` сразу | В фоне обходит все незавершённые слоты карты в текущем цикле |
| `/cron/toloka/ParseLatest` | `pages` (5) | лог `<раздел> - <страница>` по успешным страницам, либо `ok`; `work`, если занято | Первые `pages` страниц каждого раздела карты; успешные отмечаются выполненными в текущем цикле |

## Рекомендуемое расписание

```crontab
# Свежие раздачи каждые 15 минут (~3 с на запуск)
14,29,44,59 * * * *  /opt/crabindex/Data/run-job.sh toloka-parse http://127.0.0.1:9117/cron/toloka/parse 900

# Пересчёт карты страниц
30 2 * * *  /opt/crabindex/Data/run-job.sh toloka-UpdateTasksParse http://127.0.0.1:9117/cron/toloka/UpdateTasksParse 60

# Полный обход (~783 страницы, 1-2 ч); новый круг - только при pending = 0
55 5,13,21 * * *  /opt/crabindex/Data/run-job.sh toloka-ParseAllTask http://127.0.0.1:9117/cron/toloka/ParseAllTask 60
```

`ParseAllTask` начинает новый круг, только когда в текущем цикле не осталось незавершённых страниц; после рестарта незавершённый цикл продолжает `/cron/maintenance/ResumeParseAll` (см. [Cron](../deployment/cron.md)). Цикл прерывается после 45 минут без прогресса.

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/toloka_taskParse.json` | Карта страниц: раздел → список слотов с датой обновления и отметкой цикла |
| `Data/temp/toloka_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- Страница раздела - 45 тем (`/f<раздел>-<page×45>`). `UpdateTasksParse` создаёт слоты `0..N-1` по последнему номеру в пейджере и удаляет слоты за последней страницей.
- Страница без признаков форума (например, стена входа) не считается успешной и не затирает данные.
- Фоновый обход уступает ежечасному `parse` между страницами; `UpdateTasksParse`, `ParseAllTask` и `ParseLatest` взаимоисключающие.
- Без действующего логина `parse` отвечает логом, но фактически ничего не сохраняет - проверяйте `Data/log/toloka.log`.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/toloka/parse"
curl "http://127.0.0.1:9117/cron/toloka/UpdateTasksParse"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
curl "http://127.0.0.1:9117/health/background-jobs"
tail -f Data/log/toloka.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Обслуживание FileDB](../operations/maintenance.md)
- [Решение проблем](../operations/troubleshooting.md)
