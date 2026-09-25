# LE-Production

![](/img/trackers/leproduction.ico)

LE-Production (slug `leproduction`) - релиз-группа с аниме, дорамами, фильмами, сериалами и мультфильмами. CrabIndex обходит шесть разделов сайта: `anime`, `dorama`, `film`, `serial`, `fulcartoon` и `cartoon`. Из листинга берутся ссылки на страницы релизов, на каждой странице разбирается каждое качество отдельно, и в FileDB попадает по одной записи на качество. Авторизация не нужна.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Leproduction` |
| `host` | `https://www.le-production.online` |
| `alias` | не задан. Если задан, запросы идут на `alias` |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс между страницами листинга |
| Авторизация | нет |
| Кодировка | UTF-8 |
| Магнет-ссылки | со страницы релиза, иначе со страницы `index.php?do=download&id=…` |
| Учитывает `disable_trackers` | нет |

Типы по разделам: `anime` → `anime`, `dorama` и `serial` → `serial`, `film` → `movie`, `fulcartoon` → `multfilm`, `cartoon` → `multserial`.

## Конфигурация

```yaml
Leproduction:
  host: https://www.le-production.online
  useproxy: false
  reqMinute: 8
  log: true
```

## Cron-маршруты

Путь и имена параметров не зависят от регистра. Маршрут принимает только `GET`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/leproduction/parse` | `limit_page` (по умолчанию `0`) | `ok` после завершения, `work` если parse уже идёт, `config missing` | Синхронно обходит страницы 1…`limit_page` каждого раздела |

Если `limit_page` ≤ 0 (в том числе когда параметр не указан), последняя страница определяется по блоку навигации раздела (ссылки `/{раздел}/page/N/` до кнопки «Дальше»), и обходится **весь** раздел.

## Рекомендуемое расписание

```crontab
45 6 * * *  /opt/crabindex/Data/run-job.sh leproduction-parse "http://127.0.0.1:9117/cron/leproduction/parse?limit_page=3" 900
```

Раз в сутки проверяются первые три страницы каждого раздела. Для первичного заполнения вызовите маршрут без `limit_page` и с большим таймаутом.

## Файлы в Data/temp

Нет.

## Особенности и ограничения

- Ссылка записи в FileDB: `{ссылка на релиз}?q={качество}&id={id торрента}`. Так каждое качество хранится отдельной записью.
- К заголовку добавляются диапазон серий (`[1-12 из 24]`), год и качество (`[1080p]`).
- Магнет со страницы загрузки запрашивается, только если на странице релиза его нет, а запись новая или изменилась.
- Страницы релизов внутри одной страницы листинга запрашиваются без задержки. `parseDelay` действует только между страницами листинга.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/leproduction/parse?limit_page=1"
tail -f /opt/crabindex/Data/log/leproduction.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
