# ViruseProject

![](/img/trackers/viruseproject.ico)

ViruseProject (slug `viruseproject`) - релиз-группа с озвучкой сериалов, фильмов, документальных фильмов, мультфильмов и реалити-шоу. CrabIndex обходит пять разделов `/releases/{раздел}?start=N`. На странице релиза разбираются вложения `.torrent`, для каждого качества создаётся отдельная запись. Магнет-ссылка строится из скачанного `.torrent`. Авторизация не нужна.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Viruseproject` |
| `host` | `https://viruseproject.tv` |
| `alias` | не задан. Если задан, запросы идут на `alias` |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс между страницами листинга |
| Авторизация | нет |
| Кодировка | UTF-8 |
| Магнет-ссылки | из `.torrent` со страницы релиза |
| Учитывает `disable_trackers` | нет |

Разделы и типы:

| Раздел | Типы | Шаг `start` |
| --- | --- | --- |
| `serials` | `serial` | 10 |
| `movies` | `movie` | 10 |
| `documentary` | `docuserial`, `documovie` | 6 |
| `cartoons` | `multfilm`, `multserial` | 6 |
| `reality-show` | `tvshow` | 6 |

## Конфигурация

```yaml
Viruseproject:
  host: https://viruseproject.tv
  useproxy: false
  reqMinute: 8
  log: true
```

## Cron-маршруты

Путь и имена параметров не зависят от регистра. Маршрут принимает только `GET`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/viruseproject/parse` | `limit_page` (по умолчанию `0`) | `ok` после завершения, `work` если parse уже идёт, `config missing` | Синхронно обходит страницы 1…`limit_page` каждого раздела |

Последняя страница раздела определяется по ссылке `pagination-end` на первой странице. Если `limit_page` ≤ 0 или больше последней страницы, обходится весь раздел.

## Рекомендуемое расписание

```crontab
50 6 * * *  /opt/crabindex/Data/run-job.sh viruseproject-parse "http://127.0.0.1:9117/cron/viruseproject/parse?limit_page=3" 900
```

Раз в сутки проверяются первые три страницы каждого раздела. Для первичного заполнения вызовите маршрут без `limit_page` и с большим таймаутом.

## Файлы в Data/temp

Нет.

## Особенности и ограничения

- Ссылка записи в FileDB: `{ссылка на релиз}#q={разрешение}&id={id вложения}`.
- `.torrent` скачивается только для новых записей, при изменившемся заголовке или пустом магнете. Если скачать не удалось, а в базе уже есть магнет, сохраняется старый.
- Страницы релизов внутри одной страницы листинга запрашиваются без задержки. `parseDelay` действует только между страницами листинга.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/viruseproject/parse?limit_page=1"
tail -f /opt/crabindex/Data/log/viruseproject.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
