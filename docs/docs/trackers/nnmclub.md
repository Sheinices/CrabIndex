# NNM-Club

![](/img/trackers/nnmclub.ico)

NNM-Club (slug `nnmclub`) - трекер около 145 тысяч раздач, работает без авторизации. CrabIndex читает ленты портала `/forum/portal.php?c=<cat>&start=<page×25>` в кодировке cp1251 и сам переводит их в UTF-8. Магнет-ссылка есть в карточке раздачи на портале, поэтому страницы тем не запрашиваются.

Входит в trio-кластер: `parse`, `UpdateTasksParse`, `ParseAllTask` (плюс `ParseLatest`).

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `NNMClub` |
| `host` | `https://nnmclub.to` |
| `alias` | не задан |
| `reqMinute` | `8` (пауза между страницами полного обхода - 7000 мс) |
| Авторизация | не требуется |
| Кодировка | windows-1251 |
| Магнет-ссылки | из листинга портала |
| Учитывает `disable_trackers` в cron | нет |

## Конфигурация

```yaml
NNMClub:
  host: https://nnmclub.to
  alias: ""        # опционально: зеркало или адрес .onion
  useproxy: false
  reqMinute: 8
  log: true
```

Ссылки на раздачи в FileDB строятся от `host` (`<host>/forum/viewtopic.php?...`), запросы уходят на `alias`, если он задан.

Для доступа через Tor укажите `.onion`-адрес в `alias` и добавьте правило `globalproxy`, например:

```yaml
globalproxy:
  - pattern: "\\.onion"
    list:
      - socks5://127.0.0.1:9050
```

Правила `globalproxy` применяются по шаблону URL сами по себе; `useproxy: true` нужен только для общего списка `proxy.list`. Подробнее - [Прокси](../configuration/proxy.md).

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Доступ - из локальной сети или с `devkey`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/nnmclub/parse` | `page` (по умолчанию `0`) | лог `<cat> - <page>` по каждой категории; `work` | Синхронно разбирает страницу `page` (смещение `start=page×25`) всех разделов |
| `/cron/nnmclub/UpdateTasksParse` | - | `ok` или `work` | Фоновое обновление карты страниц по пейджеру портала |
| `/cron/nnmclub/ParseAllTask` | - | `ok` или `work` | Фоновый полный обход карты |
| `/cron/nnmclub/ParseLatest` | `pages` (по умолчанию `5`) | строки `<cat> - <page>`, `ok` или `work` | Первые `pages` страниц каждого раздела из карты |

Разделы портала: новинки кино (10), наше кино (13), зарубежное кино (6), HD/UHD/3D (11), наши и зарубежные сериалы (4, 3), документальное (22, 23), аниме (1), детям и родителям (7 - только мультфильмы, без PDF-книг), спорт (24), театр и разное (21), юмор (27).

## Рекомендуемое расписание

Из `Data/crontab`:

```crontab
# свежие раздачи каждые 15 минут
5,20,35,50 * * * *  /opt/crabindex/Data/run-job.sh nnmclub-parse http://127.0.0.1:9117/cron/nnmclub/parse 900

# обновление карты раз в сутки
15 2 * * *  /opt/crabindex/Data/run-job.sh nnmclub-UpdateTasksParse http://127.0.0.1:9117/cron/nnmclub/UpdateTasksParse 60

# полный обход (~7 ч) раз в сутки, после утренней волны
42 12 * * *  /opt/crabindex/Data/run-job.sh nnmclub-ParseAllTask http://127.0.0.1:9117/cron/nnmclub/ParseAllTask 60
```

`ParseAllTask` начинает новый круг только при pending = 0; после рестарта цикл продолжает `/cron/maintenance/ResumeParseAll`.

## Файлы в Data/temp

| Файл | Содержимое |
| --- | --- |
| `Data/temp/nnmclub_taskParse.json` | Карта страниц портала по разделам |
| `Data/temp/nnmclub_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- Портал NNM-Club отдаёт только первые 500 страниц раздела, дальше перенаправляет на FAQ-тему об этом ограничении. Поэтому карта страниц обрезается до 500 (страницы 0-499), а более старые раздачи через портал недоступны.
- Страница с FAQ об ограничении или пустая лента портала считаются пройденными - повторно в том же цикле они не запрашиваются. Ответ без `NNM-Club</title>` (ошибка сети, заглушка) - временная ошибка, страница пробуется снова.
- Число страниц берётся из цифры перед ссылкой «След.» в пейджере (без пейджера - одна страница). Если раздел не ответил страницей NNM-Club, `UpdateTasksParse` его пропускает; существующие слоты не удаляются (новые лишь дописываются, обрезка идёт только по лимиту 500).
- `ParseAllTask`/`ParseLatest` уступают часовому `parse` и выдерживают паузу `parseDelay` между страницами.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/nnmclub/parse"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
tail -f /opt/crabindex/Data/log/nnmclub.log
```

Строка `portal limit FAQ` в логе - нормальный признак конца доступного окна портала.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
