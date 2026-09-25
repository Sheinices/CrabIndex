# SubsPlease

![](/img/trackers/subsplease.ico)

SubsPlease (slug `subsplease`) - релиз-группа аниме с публичным JSON API, авторизация не нужна. CrabIndex сохраняет только варианты **1080p**, все записи получают тип `anime` и качество 1080.

Задач две. `parse` читает ленту последних релизов (`/api/?f=latest`). `ParseShows` проходит каталог шоу через `/api/?f=show`, так в базу попадают и батчи (Batch). Позиция в каталоге сохраняется между запусками.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `SubsPlease` |
| `host` | `https://subsplease.org` |
| `alias` | не задан |
| `reqMinute` | `8` → `parseDelay` 7000 мс; в `Data/example.yaml` - `30` → 2000 мс |
| Авторизация | не нужна |
| Кодировка | JSON, UTF-8 |
| Магнет-ссылки | прямо из API (только 1080p) |
| Учитывает `disable_trackers` | нет (при пустом `host` маршруты отвечают `disabled`) |

## Конфигурация

```yaml
SubsPlease:
  host: https://subsplease.org
  useproxy: false
  reqMinute: 30
  log: true
```

Пауза `parseDelay` действует между страницами ленты и между шоу в `ParseShows`. Она вычисляется из `reqMinute`, поэтому `parseDelay` из `init.yaml` игнорируется.

## Cron-маршруты

Маршруты принимают только `GET`. Регистр в путях и именах параметров не важен.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/subsplease/parse` | `pages` (по умолчанию `2`; ≤ 0 → 2; не больше 50) | `ok`; `work`, если уже идёт; `disabled` при пустом `host` | Синхронно читает `pages` страниц ленты `f=latest`. Если ответ пустой или API вернул `limit_reached`, чтение прекращается |
| `/cron/subsplease/ParseShows` | `limit` (по умолчанию `50`; ≤ 0 → 50; не больше 200), `reset` (`false`) | `ok`; `work`; `disabled` | Обрабатывает следующие `limit` шоу каталога, начиная с сохранённого курсора. `reset=true` сбрасывает контрольную точку |
| `/cron/subsplease/ParseShowStatus` | - | JSON (см. ниже) | Сводка по контрольной точке `ParseShows` |

Порядок в `ParseShows` такой: сначала шоу из текущего расписания (`f=schedule`), затем весь каталог `/shows/`, без повторов. Если для шоу ещё не известен `sid`, он берётся со страницы `/shows/<slug>/` и запоминается. Когда курсор доходит до конца каталога, он сбрасывается в 0, и следующий запуск начинает заново.

Пример ответа `ParseShowStatus`:

```json
{
  "ok": true,
  "updatedAt": "2026-09-25T05:41:12.1234567Z",
  "cursor": 150,
  "shows": 612,
  "withSid": 150,
  "schedulePriority": 48,
  "sample": [
    { "slug": "one-piece", "sid": "123", "batchCount": 0, "episodeCount": 12, "lastFetched": "2026-09-25T05:25:03.0000000Z" }
  ]
}
```

## Рекомендуемое расписание

```crontab
# свежие релизы - 2 страницы раз в час
35 * * * *  /opt/crabindex/Data/run-job.sh subsplease-parse "http://127.0.0.1:9117/cron/subsplease/parse?pages=2" 900

# каталог шоу и батчи - по 50 шоу в сутки (limit по умолчанию)
25 5 * * *  /opt/crabindex/Data/run-job.sh subsplease-ParseShows http://127.0.0.1:9117/cron/subsplease/ParseShows 1800
```

`parse` и `ParseShows` держат разные блокировки и могут работать одновременно.

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/subsplease_shows.json` | Контрольная точка `ParseShows`: курсор, список шоу с `sid`, счётчики эпизодов и батчей, последние info-hash 1080p |

## Особенности и ограничения

- Всё, кроме 1080p, отбрасывается. Если у релиза нет варианта 1080p, он в базу не попадает.
- URL записи в FileDB: `{host}/shows/<slug>/?ep=<эпизод>&res=1080`. Идентификатор раздачи вычисляется из номера эпизода.
- Запись обновляется, если изменились магнет, заголовок или размер. Иначе она пропускается.
- При 50 шоу в сутки полный проход по каталогу из нескольких сотен шоу занимает около двух недель. Чтобы пройти быстрее, увеличьте `limit` (до 200) или вызывайте `ParseShows` вручную несколько раз.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/subsplease/parse?pages=1"
curl "http://127.0.0.1:9117/cron/subsplease/ParseShows?limit=10"
curl "http://127.0.0.1:9117/cron/subsplease/ParseShowStatus"
tail -f Data/log/subsplease.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
