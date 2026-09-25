# Rutracker

![](/img/trackers/rutracker.ico)

Rutracker (slug `rutracker`) - самый большой источник в базе CrabIndex, около 1,5 млн раздач. Сайт закрыт Cloudflare, поэтому для работы нужен FlareSolverr (решает challenge и держит `cf_clearance` в браузерной сессии) и, желательно, cffetch - быстрые GET-запросы с TLS-отпечатком Chrome после того, как challenge решён.

CrabIndex обходит форумы `viewforum.php?f=<id>` по карте разделов (242 форума, из них 98 участвуют в ежечасном быстром парсинге). Магнет-ссылка берётся со страницы темы, поэтому каждая новая или изменённая раздача стоит один дополнительный запрос.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Rutracker` |
| `host` | `https://rutracker.org` |
| `alias` | не задан (запросы идут на `host`) |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс; в `example.yaml` - `30` (2000 мс) |
| `topicFetchAttempts` | `5` - попыток получить магнет со страницы темы (`<= 0` → 1) |
| `useproxy` | `false` |
| Авторизация | не нужна |
| Cloudflare | да: FlareSolverr + cffetch (или `alias` на Worker) |
| Кодировка | определяется по `charset` ответа, перекодирование автоматическое |
| Магнет-ссылки | со страницы темы (`viewtopic.php`) |
| Учитывает `disable_trackers` | нет - cron-маршруты работают и для отключённого трекера (из выдачи поиска он при этом исключается) |

## Конфигурация

```yaml
flaresolverr:
  enable: true
  url: http://127.0.0.1:8191/v1        # ежечасный parse
  crawlUrl: http://127.0.0.1:8193/v1   # ParseAllTask / UpdateTasksParse / ParseLatest; пусто - тот же url
  maxTimeoutMs: 300000
  sessionIdleMinutes: 120
  browserTimeoutRetries: 1
  recycleAfterTimeouts: 3
  guardedHours: 6
  recheckMinutes: 30

cffetch:
  enable: true
  url: http://127.0.0.1:8192/fetch     # образ ghcr.io/jacred-fdb/cffetch
  impersonate: chrome136
  timeoutSeconds: 25
  maxConcurrent: 4
  clearanceMinutes: 60
  proxy: socks5://127.0.0.1:20001      # тот же SOCKS, что у FlareSolverr (PROXY_URL)

Rutracker:
  host: https://rutracker.org
  alias: ""               # Worker/onion, если FlareSolverr выключен; URL в FileDB остаются на host
  useproxy: false
  reqMinute: 30           # 2000 мс между запросами
  topicFetchAttempts: 5
  log: true
```

Значения по умолчанию для блока `flaresolverr`: `enable: true`, `url: http://127.0.0.1:8191/v1`, `crawlUrl: ""`, `maxTimeoutMs: 300000`, `sessionIdleMinutes: 120`, `browserTimeoutRetries: 1`, `recycleAfterTimeouts: 3`, `guardedHours: 6`, `recheckMinutes: 30`. Для `cffetch`: `enable: true`, `url: http://127.0.0.1:8192/fetch`, `impersonate: chrome136`, `timeoutSeconds: 25`, `maxConcurrent: 4`, `clearanceMinutes: 60`, `proxy: ""`.

:::note[Примечание]
`reqMinute: 8` для полного обхода (~16 тыс. страниц) слишком медленный, а слишком частые запросы провоцируют Cloudflare отозвать `cf_clearance`. Рабочее значение - `30`.
:::

### Как CrabIndex работает с Cloudflare

1. Обычный HTTP-запрос к `rutracker.org` получает `403/503` с заголовком `cf-mitigated` (или страницу-заглушку challenge).
2. Хост помечается как «защищённый» на `flaresolverr.guardedHours` часов: дальнейшие запросы к нему сразу идут через FlareSolverr.
3. FlareSolverr решает challenge в собственной сессии Chromium. Имя сессии строится из хоста: `crabindex-rutracker_org`.
4. Если cffetch включён, после решения страницы забираются им с полученным `cf_clearance` (быстро, без браузера); браузер остаётся запасным путём.

Ежечасный `parse` ходит на `flaresolverr.url`, а фоновые задачи (`ParseAllTask`, `UpdateTasksParse`, `ParseLatest`) - на `flaresolverr.crawlUrl`, если он задан. Второй экземпляр FlareSolverr нужен, чтобы длинный архивный обход не занимал браузер, которым пользуется свежий парсинг.

### Режим без FlareSolverr

Если FlareSolverr недоступен, задайте `alias` - адрес Cloudflare Worker или зеркала, через которое пойдут запросы. В FileDB ссылки всё равно сохраняются с `host`.

```yaml
flaresolverr:
  enable: false

Rutracker:
  host: https://rutracker.org
  alias: https://your-worker.example.workers.dev
```

:::warning[Внимание]
Без FlareSolverr и без `alias` каждый запрос к `rutracker.org` будет получать 403 от Cloudflare.
:::

## Cron-маршруты

Все маршруты принимают `GET` и `POST`. Пути и имена параметров регистронезависимы (`/cron/rutracker/parsealltask` = `/cron/rutracker/ParseAllTask`). Доступ - из локальной сети или с `devkey`, см. [Матрицу доступа](../operations/access-matrix.md).

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/rutracker/parse` | `page` (0), `cat` (список id форумов через запятую), `maxTopics` (0 - без ограничения) | текстовый лог `<форум> - <страница> - True/False` по строке на форум; `work`, если parse уже идёт | Синхронно разбирает страницу `page` каждого «быстрого» форума. `cat` сужает список; если в нём нет быстрых форумов, используются любые известные id из карты. `maxTopics` ограничивает число тем (и запросов к темам) на страницу - удобно для проверки |
| `/cron/rutracker/UpdateTasksParse` | `cat` (необязательно) | `ok` / `work` сразу | В фоне перечитывает пейджер каждого форума и перестраивает карту страниц. Лимит - 30 минут |
| `/cron/rutracker/ParseAllTask` | `cat`, `maxPages` (0) | `ok` / `work` сразу | В фоне обходит все незавершённые слоты карты. Без параметров - полный цикл; с `cat` или `maxPages > 0` - частичный прогон без смены цикла (слоты, не обновлённые сегодня) |
| `/cron/rutracker/ParseLatest` | `pages` (5) | лог `<форум> - <страница>` по успешным страницам, либо `ok`; `work`, если занято | Первые `pages` страниц каждого форума из карты; успешные страницы отмечаются выполненными в текущем цикле |

Смежные маршруты: `GET /cron/cloudflare/Warmup` (прогрев сессии FlareSolverr, по умолчанию открывает `https://rutracker.org/forum/tracker.php?nm=`, параметр `url` - другой адрес), `/cron/maintenance/ParseAllStatus`, `/cron/maintenance/ResumeParseAll`. См. [Cron API](../api-reference/cron.md).

Ответ прогрева:

```json
{"ok": true, "host": "rutracker.org", "length": 123456, "tookSeconds": 11.3}
```

## Рекомендуемое расписание

```crontab
# Прогрев сессии FlareSolverr каждые 20 минут
5,25,45 * * * *  /opt/crabindex/Data/run-job.sh cloudflare-keepalive http://127.0.0.1:9117/cron/cloudflare/Warmup 300

# Прогрев перед ежечасным parse
55 * * * *  /opt/crabindex/Data/run-job.sh cloudflare-warmup http://127.0.0.1:9117/cron/cloudflare/Warmup 300

# Свежие раздачи - каждый час в :00
0 * * * *  /opt/crabindex/Data/run-job.sh rutracker-parse http://127.0.0.1:9117/cron/rutracker/parse 3600

# Пересчёт карты страниц
20 3 * * *  /opt/crabindex/Data/run-job.sh rutracker-UpdateTasksParse http://127.0.0.1:9117/cron/rutracker/UpdateTasksParse 60

# Полный обход: многодневный цикл (~16 тыс. страниц); новый круг - только при pending = 0
40 4 * * *  /opt/crabindex/Data/run-job.sh rutracker-ParseAllTask http://127.0.0.1:9117/cron/rutracker/ParseAllTask 60

# Продолжение незавершённых циклов всех трекеров после рестарта
*/15 * * * *  /opt/crabindex/Data/run-job.sh parseall-resume http://127.0.0.1:9117/cron/maintenance/ResumeParseAll 60
```

- Ежечасный `parse` - главный источник свежести; фоновый `ParseAllTask` уступает ему между страницами и не блокирует его.
- Ежедневный запуск `ParseAllTask` начинает новый круг только когда в текущем цикле не осталось незавершённых страниц; пока цикл идёт, вызов возвращает `work`. Дополнительные запуски не нужны.
- Цикл не ограничен по времени: он идёт до `pending = 0`, остановки сервиса или 45 минут без прогресса (сторожевой таймер). После рестарта его продолжает `ResumeParseAll`.

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/rutracker_taskParse.json` | Карта страниц: форум → список слотов (страниц) с датой обновления и отметкой цикла |
| `Data/temp/rutracker_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` (id цикла, время старта, отпечаток и размер карты) |

## Особенности и ограничения

- Страница форума - 50 тем (`start = page × 50`). `UpdateTasksParse` берёт число страниц из «Страница 1 из N» и создаёт слоты `0..N-1`; слоты за последней страницей удаляются. Без `cat` из карты также удаляются форумы, которых больше нет в списке разделов.
- Пустой ответ или страница Cloudflare при `UpdateTasksParse` не обнуляет карту - такой форум просто пропускается.
- «Опустошённые» архивные разделы, где пейджер показывает много страниц, но тем нет, считаются одной страницей.
- Раздел может содержать и собственные темы, и подразделы: обход родителя не заменяет обход детей, поэтому в карте перечислены и дочерние форумы (например, UHD-разделы сериалов).
- Страница темы не запрашивается повторно, если заголовок и размер совпадают с сохранённой записью, магнет уже есть и запись обновлена не раньше даты из листинга. Иначе делается до `topicFetchAttempts` попыток; перед каждой выдерживается `parseDelay`.
- Слот цикла закрывается при успехе или после трёх неудачных попыток подряд (в логе: `ParseAll skip slot page=… after 3 failures`); следующий цикл снова попробует такие страницы.
- `UpdateTasksParse`, `ParseAllTask` и `ParseLatest` взаимоисключающие; не запускайте их вручную одновременно.
- Процент в `/health/background-jobs` для `ParseAllTask` - это доля обойдённых слотов, а не число новых магнитов.

## Диагностика

```bash
# Прогрев и проверка сессии FlareSolverr
curl "http://127.0.0.1:9117/cron/cloudflare/Warmup"

# Ручной parse одного форума с ограничением тем
curl "http://127.0.0.1:9117/cron/rutracker/parse?page=0&cat=1669&maxTopics=20"

# Пересчёт карты только для одного форума
curl "http://127.0.0.1:9117/cron/rutracker/UpdateTasksParse?cat=1669"

# Фоновые задачи и циклы ParseAll
curl "http://127.0.0.1:9117/health/background-jobs"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
curl "http://127.0.0.1:9117/cron/maintenance/ResumeParseAll"

# Лог трекера
tail -f Data/log/rutracker.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [FlareSolverr и cffetch](../configuration/flaresolverr.md)
- [Прокси](../configuration/proxy.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Обслуживание FileDB](../operations/maintenance.md)
- [Решение проблем](../operations/troubleshooting.md)
