# Torrent.by

![](/img/trackers/torrentby.ico)

Torrent.by (slug `torrentby`) - белорусский трекер около 58 тысяч раздач, работает без авторизации. CrabIndex читает разделы `/<раздел>/?page=<N>`; магнет-ссылка есть в строке листинга, поэтому страницы раздач не запрашиваются. Входит в trio-кластер.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `TorrentBy` |
| `host` | `https://torrent.by` |
| `alias` | не задан |
| `reqMinute` | `8` (пауза между страницами - 7000 мс) |
| Авторизация | не требуется |
| Кодировка | UTF-8 |
| Магнет-ссылки | из листинга |
| Учитывает `disable_trackers` в cron | нет |

## Конфигурация

```yaml
TorrentBy:
  host: https://torrent.by
  alias: ""        # например, https://torrentby.<account>.workers.dev
  useproxy: false
  reqMinute: 8
  log: true
```

Если `torrent.by` недоступен из вашей сети, поднимите прокси-зеркало (например, Cloudflare Worker, см. `alias` в [настройке трекеров](../configuration/trackers.md)) и укажите его в `alias`. Запросы пойдут на `alias`, а ссылки на раздачи в FileDB останутся на `host`.

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Доступ - из локальной сети или с `devkey`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/torrentby/parse` | `page` (по умолчанию `0`) | лог `<раздел> - <page>`; `work` | Синхронно разбирает страницу `page` всех разделов |
| `/cron/torrentby/UpdateTasksParse` | - | `ok` или `work` | Фоновое обновление карты страниц |
| `/cron/torrentby/ParseAllTask` | - | `ok` или `work` | Фоновый полный обход карты |
| `/cron/torrentby/ParseLatest` | `pages` (по умолчанию `5`) | строки `<раздел> - <page>`, `ok` или `work` | Первые `pages` страниц каждого раздела из карты |

Разделы: `films`, `movies` (фильмы), `serials`, `series` (сериалы), `tv`, `humor` (ТВ и юмор), `cartoons` (мультфильмы), `anime`, `sport`.

## Рекомендуемое расписание

Из `Data/crontab`:

```crontab
# свежие раздачи каждые 15 минут
7,22,37,52 * * * *  /opt/crabindex/Data/run-job.sh torrentby-parse http://127.0.0.1:9117/cron/torrentby/parse 900

# обновление карты раз в сутки
25 2 * * *  /opt/crabindex/Data/run-job.sh torrentby-UpdateTasksParse http://127.0.0.1:9117/cron/torrentby/UpdateTasksParse 60

# полный обход (~197 страниц, минуты) каждые 6 часов
50 5,11,17,23 * * *  /opt/crabindex/Data/run-job.sh torrentby-ParseAllTask http://127.0.0.1:9117/cron/torrentby/ParseAllTask 60
```

`ParseAllTask` начинает новый круг только при pending = 0; после рестарта цикл продолжает `/cron/maintenance/ResumeParseAll`.

## Файлы в Data/temp

| Файл | Содержимое |
| --- | --- |
| `Data/temp/torrentby_taskParse.json` | Карта страниц по разделам |
| `Data/temp/torrentby_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- Пейджер сайта показывает не все номера, а обрывается на `...`. `UpdateTasksParse` проходит по этим переходам (до 40 прыжков на раздел, с паузой `parseDelay`), а затем двоичным поиском отбрасывает пустой хвост: последней считается последняя страница, где реально есть раздачи.
- Слоты за найденной последней страницей удаляются из карты. Если раздел не ответил, он пропускается - карта не обнуляется. После изменений пагинации на сайте достаточно один раз вызвать `UpdateTasksParse` вручную.
- `ParseAllTask`/`ParseLatest` уступают часовому `parse` и выдерживают паузу `parseDelay` между страницами.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/torrentby/parse"
curl "http://127.0.0.1:9117/cron/torrentby/UpdateTasksParse"
tail -f /opt/crabindex/Data/log/torrentby.log
```

В логе `UpdateTasksParse cat=<раздел>: maxPage=…, total=…` показывает итог по разделу; `pagerWas=…` - сколько страниц заявлял пейджер до отсечения пустого хвоста.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
