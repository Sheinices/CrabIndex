# Ultradox

![](/img/trackers/ultradox.ico)

Ultradox (slug `ultradox`) - открытый трекер фильмов, сериалов и аниме, авторизация не нужна. CrabIndex обходит шесть разделов сайта: `serial-hd`, `hd`, `rufilm`, `camrip`, `webrips` и `anime`. Используется полный набор задач: частый `parse`, ежесуточный `UpdateTasksParse`, возобновляемый `ParseAllTask` и `ParseLatest` для первых страниц.

В листинге Ultradox магнет-ссылки пустые (`btih` без хэша), поэтому для каждой строки CrabIndex открывает страницу раздачи и берёт с неё настоящие магнеты. Если на странице несколько вариантов (разное качество), каждый сохраняется отдельной записью. Главное требование сайта - `Referer`, похожий на поисковую выдачу. С Referer собственного домена nginx отвечает `503`.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Ultradox` |
| `host` | `https://ultradox.vip` |
| `alias` | не задан |
| `reqMinute` | `8` → пауза `parseDelay` 7000 мс |
| Авторизация | не нужна |
| Кодировка | UTF-8 |
| Магнет-ссылки | со страницы раздачи (отдельный GET на каждую строку листинга) |
| Учитывает `disable_trackers` | да, все cron-маршруты отвечают `disabled` |

## Конфигурация

```yaml
Ultradox:
  host: https://ultradox.vip
  useproxy: false
  reqMinute: 8
  log: true
```

`parseDelay` вычисляется из `reqMinute`, задавать его в файле бесполезно: значение из `init.yaml` игнорируется.

:::note[Примечание]
`https://ultradox.vip` отвечает редиректом `307` на номерное зеркало вида `00N.ultradox.vip`, и HTTP-клиент следует за редиректом сам. Не прописывайте конкретное зеркало ни в `host`, ни в `alias`: номер меняется. URL в FileDB строятся от адреса, по которому идут запросы (`alias`, если он задан, иначе `host`), поэтому с зеркалом в `alias` в базе окажутся ссылки на зеркало.
:::

### Referer

CrabIndex отправляет `Referer: https://www.google.com/` и набор браузерных заголовков (`Sec-Fetch-*`, `Accept-Language` и т. п.). Если перед Ultradox стоит ваш прокси или CDN, он не должен подменять `Referer`. Если хост попал под Cloudflare и запросы идут через FlareSolverr или cffetch, туда передаётся тот же Referer (см. [FlareSolverr и cffetch](../configuration/flaresolverr.md)).

## Cron-маршруты

Регистр в путях и именах параметров не важен.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/ultradox/parse` | `page` (по умолчанию `0`) | построчный лог `раздел - page` или `ok`; `work`, если parse уже идёт; `config missing`, если `host` пуст | Синхронно разбирает страницу `page` в каждом из шести разделов. `page` ≤ 0 - корень раздела, иначе `/<раздел>/page/<page>/` |
| `/cron/ultradox/UpdateTasksParse` | - | `ok` / `work` / `disabled` сразу | В фоне читает пейджер каждого раздела и обновляет карту страниц (слоты `1..maxPage`). Слоты за последней страницей удаляются. Лимит - 30 минут |
| `/cron/ultradox/ParseAllTask` | - | `ok` / `work` / `disabled` сразу | Фоновый полный обход карты. Новый круг начинается только при pending = 0. Если карта пуста, она сначала строится |
| `/cron/ultradox/ParseLatest` | `pages` (по умолчанию `5`, значение ≤ 0 тоже даёт 5) | построчный лог или `ok`; `work` | Разбирает первые `pages` страниц каждого раздела из карты и отмечает их выполненными в текущем цикле ParseAll |

## Рекомендуемое расписание

```crontab
# parse - каждые 30 минут (:13 и :43, в стороне от rutor 1/16/31/46; идёт ~5 мин)
13,43 * * * *  /opt/crabindex/Data/run-job.sh ultradox-parse http://127.0.0.1:9117/cron/ultradox/parse 900

# обновление карты страниц
50 2 * * *  /opt/crabindex/Data/run-job.sh ultradox-UpdateTasksParse http://127.0.0.1:9117/cron/ultradox/UpdateTasksParse 60

# полный обход (~9 ч) - раз в сутки, вместе с nnmclub, после утренней волны
58 12 * * *  /opt/crabindex/Data/run-job.sh ultradox-ParseAllTask http://127.0.0.1:9117/cron/ultradox/ParseAllTask 60

# первые страницы разделов
20 7 * * *  /opt/crabindex/Data/run-job.sh ultradox-ParseLatest http://127.0.0.1:9117/cron/ultradox/ParseLatest 900
```

Если процесс перезапустился посреди цикла, обход продолжит общий `/cron/maintenance/ResumeParseAll` (в `Data/crontab` он стоит каждые 15 минут). Если 45 минут нет прогресса, ParseAll останавливается сам.

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/ultradox_taskParse.json` | Карта страниц по разделам |
| `Data/temp/ultradox_parseAllCycle.json` | Состояние текущего цикла ParseAll |

## Особенности и ограничения

- Каждая строка листинга стоит одного дополнительного GET, и перед каждым из них выдерживается `parseDelay`. Поэтому одна страница раздела занимает минуты, а полный обход - около 9 часов.
- В `UpdateTasksParse` последняя страница раздела берётся как **наименьшее** значение среди пейджеров `div.pages` этого раздела: нижний пейджер DLE завышен (в хвосте пустые страницы). Числа из `<script>` не учитываются. Пустой ответ карту не обнуляет.
- Если листинг не загрузился, слот ParseAll не считается выполненным: он закрывается после трёх неудач подряд, а в следующем круге его снова попробуют.
- Запись обновляется, только если изменились заголовок или магнет. Без изменений строка пропускается.
- Если в базе остались дубли со старых доменов (`ultradox.onl`, `00N.ultradox.vip`), сделайте бэкап `Data/fdb/` и вызовите `/dev/FixUltradoxDomainDuplicates`. Он переписывает URL на канонический `host` и сливает дубли (см. [Обслуживание FileDB](../operations/maintenance.md)).

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/ultradox/parse"
curl "http://127.0.0.1:9117/cron/ultradox/ParseLatest?pages=2"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
tail -f Data/log/ultradox.log
```

Если в логе `Listing fetch failed` или `fetched=0`, проверьте, что `Referer` не подменяется по пути к сайту, и что зеркало отвечает.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Обслуживание FileDB](../operations/maintenance.md)
- [Решение проблем](../operations/troubleshooting.md)
