# Megapeer

![](/img/trackers/megapeer.ico)

Megapeer (slug `megapeer`) - трекер без обязательной авторизации, около 113 тысяч раздач. CrabIndex читает листинги `browse.php?cat=<cat>&page=<N>` в кодировке cp1251 и сам переводит их в UTF-8. Магнет-ссылки в листинге нет, поэтому для новых или изменившихся раздач CrabIndex скачивает `.torrent` (`/download/<id>`) и строит magnet по его info-hash.

Megapeer жёстко ограничивает частоту запросов, поэтому у трекера своя схема пауз: перед каждым запросом листинга выдерживается 30, 60 или 90 секунд по кругу. Входит в trio-кластер.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Megapeer` |
| `host` | `http://megapeer.vip` |
| `alias` | не задан |
| `reqMinute` | `5` (вычисленный `parseDelay` - 12000 мс, но парсер его не использует, см. ниже) |
| Авторизация | не требуется |
| Кодировка | windows-1251 |
| Магнет-ссылки | из скачанного `.torrent` |
| Учитывает `disable_trackers` в cron | да - для отключённого трекера маршруты отвечают `disabled` |

## Конфигурация

```yaml
Megapeer:
  host: http://megapeer.vip
  alias: ""
  useproxy: false
  reqMinute: 5
  log: true
```

- Листинги запрашиваются через `alias` (если задан) с учётом `useproxy`.
- `.torrent` скачивается всегда с `host` и без `proxy.list` (правила `globalproxy` действуют по шаблону URL).
- `reqMinute`/`parseDelay` на темп обхода Megapeer не влияют: паузы 30/60/90 с встроены в загрузчик листинга.

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Доступ - из локальной сети или с `devkey`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/megapeer/parse` | `page` (по умолчанию `0`) | лог `<cat> - <page> / True` или `False`; `work`; `disabled` | Синхронно разбирает страницу `page` всех категорий |
| `/cron/megapeer/UpdateTasksParse` | - | `ok`, `work` или `disabled` | Фоновое обновление карты страниц |
| `/cron/megapeer/ParseAllTask` | - | `ok`, `work` или `disabled` | Фоновый полный обход карты |
| `/cron/megapeer/ParseLatest` | `pages` (по умолчанию `5`) | строки `<cat> - <page>`, `ok`, `work` или `disabled` | Первые `pages` страниц каждой категории из карты |

Категории: зарубежные и наши фильмы (80, 79), сериалы (6, 5), документальное (55), ТВ-шоу (57), мультфильмы (76).

## Рекомендуемое расписание

Из `Data/crontab`:

```crontab
# свежие раздачи раз в час
8 * * * *  /opt/crabindex/Data/run-job.sh megapeer-parse http://127.0.0.1:9117/cron/megapeer/parse 900

# обновление карты раз в сутки
20 2 * * *  /opt/crabindex/Data/run-job.sh megapeer-UpdateTasksParse http://127.0.0.1:9117/cron/megapeer/UpdateTasksParse 60

# полный обход (~77 страниц, ~1,3 ч) трижды в сутки
45 5,13,21 * * *  /opt/crabindex/Data/run-job.sh megapeer-ParseAllTask http://127.0.0.1:9117/cron/megapeer/ParseAllTask 60
```

Один `parse` - это 7 категорий с паузой 30-90 с перед каждой, то есть несколько минут; поэтому он запускается раз в час, а не каждые 15 минут. `ParseAllTask` начинает новый круг только при pending = 0; после рестарта цикл продолжает `/cron/maintenance/ResumeParseAll`.

## Файлы в Data/temp

| Файл | Содержимое |
| --- | --- |
| `Data/temp/megapeer_taskParse.json` | Карта страниц по категориям |
| `Data/temp/megapeer_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- Карта страниц ограничена: последняя страница считается как «Всего: N» / 50, но не больше 10. На категорию в карте максимум 11 страниц (0-10), всего около 77. Более старые раздачи полным обходом не достигаются.
- Одновременно выполняется только один запрос листинга. Если листинг уже загружается (например, идёт `ParseAllTask`), параллельный запрос сразу пропускается и страница считается неудачной.
- Каждый листинг пробуется до 3 раз. Страница засчитывается, только если в ответе есть разметка сайта (`id="logo"`).
- `.torrent` не скачивается повторно, если раздача уже есть в FileDB с тем же заголовком.
- `UpdateTasksParse` удаляет слоты за последней страницей. Пустой ответ карту не меняет.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/megapeer/parse"
curl "http://127.0.0.1:9117/health/background-jobs"
tail -f /opt/crabindex/Data/log/megapeer.log
```

Сообщение `Rate limit or invalid page ... retry` в логе значит, что сайт отдал ограничение частоты или страницу-заглушку; парсер повторит запрос после следующей паузы.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
