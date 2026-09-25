# FlareSolverr и cffetch

Часть трекеров (в первую очередь Rutracker и Kinozal) закрыта проверкой Cloudflare. Обычный HTTP-клиент её не проходит: Cloudflare проверяет браузерный TLS-отпечаток и выдаёт cookie `cf_clearance` только настоящему браузеру. CrabIndex решает это двумя внешними сервисами:

| Сервис | Роль | Порт по умолчанию |
| --- | --- | --- |
| [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) | Запускает Chromium, **решает** challenge и отдаёт cookie и User-Agent | `8191` (второй экземпляр - `8193`) |
| cffetch (`ghcr.io/jacred-fdb/cffetch`) | Быстро **скачивает** страницы с TLS-отпечатком Chrome (curl_cffi), используя полученную cookie | `8192` |

Скачивание через cffetch занимает доли секунды против 0,5-1,2 секунды у браузерного перехода, а FlareSolverr нужен только для получения и обновления cookie.

## Как это работает

1. Обычный запрос CrabIndex к трекеру получил `403`/`503` с заголовком `cf-mitigated` или страницу «Just a moment…». Хост помечается как «защищённый».
2. Для защищённого хоста CrabIndex сначала пробует cffetch: с сохранённой `cf_clearance` или, если её ещё нет, просто с TLS-отпечатком Chrome.
3. Если cffetch недоступен или снова получил challenge, CrabIndex просит FlareSolverr открыть страницу в браузере, забирает из ответа cookie и User-Agent и запоминает их.
4. Следующие запросы снова идут через cffetch с новой cookie. Заголовки `Referer`, `Accept` и `Accept-Language` передаются только в cffetch (FlareSolverr их не поддерживает).
5. Хост остаётся «защищённым» `guardedHours` часов; раз в `recheckMinutes` минут CrabIndex пробует обычный запрос - вдруг защиту сняли.

У каждого хоста своя сессия браузера в FlareSolverr: `crabindex-rutracker_org`, `crabindex-kinozal_guru` и т. д.

Если ответ `403`/`503` пришёл без признаков Cloudflare, FlareSolverr не поможет. CrabIndex один раз пишет в лог: `{host}: 403 без признаков Cloudflare - браузерный fallback не сработает; проверьте Referer/зеркало/лимит`.

## Конфигурация

```yaml
flaresolverr:
  enable: true
  url: http://127.0.0.1:8191/v1
  crawlUrl: http://127.0.0.1:8193/v1
  maxTimeoutMs: 300000
  sessionIdleMinutes: 120
  browserTimeoutRetries: 1
  recycleAfterTimeouts: 3
  guardedHours: 6
  recheckMinutes: 30

cffetch:
  enable: true
  url: http://127.0.0.1:8192/fetch
  impersonate: chrome136
  timeoutSeconds: 25
  maxConcurrent: 4
  clearanceMinutes: 60
  proxy: socks5://127.0.0.1:20001
```

### flaresolverr

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `enable` | `true` | Использовать FlareSolverr. При `false` защищённые хосты недоступны - используйте `alias` трекера (зеркало, Worker, onion) |
| `url` | `http://127.0.0.1:8191/v1` | API FlareSolverr для обычных запросов (ежечасный `parse`, прогрев) |
| `crawlUrl` | пусто | Второй экземпляр FlareSolverr для фоновых `ParseAllTask`, `UpdateTasksParse`, `ParseLatest`. Пусто - тот же `url`. Один экземпляр обрабатывает запросы последовательно, поэтому реальный прирост даёт именно второй экземпляр |
| `maxTimeoutMs` | `300000` | Сколько FlareSolverr может решать challenge, мс |
| `sessionIdleMinutes` | `120` | Сколько держать сессию браузера без запросов |
| `browserTimeoutRetries` | `1` | Повторов в той же сессии после таймаута браузера |
| `recycleAfterTimeouts` | `3` | Пересоздавать сессию только после N таймаутов подряд |
| `guardedHours` | `6` | Сколько часов хост считается защищённым после challenge |
| `recheckMinutes` | `30` | Как часто пробовать защищённый хост обычным запросом |

### cffetch

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `enable` | `true` | Использовать cffetch. При `false` или если сервис не запущен, все запросы к защищённым хостам идут через браузер |
| `url` | `http://127.0.0.1:8192/fetch` | Адрес cffetch |
| `impersonate` | `chrome136` | Какой браузер имитировать. Не новее версии Chromium во FlareSolverr |
| `timeoutSeconds` | `25` | Таймаут запроса, секунды |
| `maxConcurrent` | `4` | Параллельных запросов через cffetch; больше - риск `429` от трекера |
| `clearanceMinutes` | `60` | Через сколько минут считать `cf_clearance` устаревшей |
| `proxy` | пусто | Прокси для cffetch. **Должен совпадать** с `PROXY_URL` FlareSolverr: `cf_clearance` привязана к IP |

## Docker Compose

Готовый стек - [docker-compose.example.yml](https://github.com/sheinices/crabindex/blob/main/docker-compose.example.yml) в репозитории: CrabIndex, два FlareSolverr (`8191` и `8193`), cffetch и WARP. FlareSolverr и cffetch работают в `network_mode: host`, чтобы выходить в интернет через один и тот же SOCKS-прокси WARP.

Адреса в `init.yaml` зависят от того, где запущен CrabIndex:

| CrabIndex | `flaresolverr.url` | `flaresolverr.crawlUrl` | `cffetch.url` |
| --- | --- | --- | --- |
| На хосте (systemd) | `http://127.0.0.1:8191/v1` | `http://127.0.0.1:8193/v1` | `http://127.0.0.1:8192/fetch` |
| В compose (bridge-сеть) | `http://host.docker.internal:8191/v1` | `http://host.docker.internal:8193/v1` | `http://host.docker.internal:8192/fetch` |

Для варианта «в compose» у сервиса CrabIndex должна быть строка `extra_hosts: ["host.docker.internal:host-gateway"]`. Адрес `http://flaresolverr:8191/v1` работает, только если FlareSolverr в той же bridge-сети (без `network_mode: host`).

:::warning[Внимание]
Не публикуйте порты `8191`, `8192`, `8193` наружу.
:::

## VPS с IP дата-центра: WARP

С IP дата-центров Cloudflare часто не выдаёт cookie даже браузеру. Решение - выпускать FlareSolverr и cffetch в интернет через Cloudflare WARP (контейнер `caomingjun/warp`, SOCKS5 на `127.0.0.1:20001`):

```yaml
# фрагмент docker-compose
  flaresolverr:
    environment:
      - PROXY_URL=socks5://127.0.0.1:20001
  cffetch:
    environment:
      - CFFETCH_PROXY=socks5://127.0.0.1:20001
      - CFFETCH_IMPERSONATE=chrome136
```

Прокси FlareSolverr задаётся переменной окружения его контейнера, а не в `init.yaml`. Сохраняйте том `/var/lib/cloudflare-warp`: иначе WARP при каждом перезапуске получает новую идентичность и IP, и проверки случаются чаще. Полный пример - [Docker](../deployment/docker.md).

## Прогрев сессии

Решение challenge занимает от десятков секунд до нескольких минут и нагружает процессор. Чтобы ежечасный парсинг Rutracker не ждал браузер, `Data/crontab` прогревает сессию заранее:

```text
5,25,45 * * * *  /opt/crabindex/Data/run-job.sh cloudflare-keepalive http://127.0.0.1:9117/cron/cloudflare/Warmup 300
55 * * * *       /opt/crabindex/Data/run-job.sh cloudflare-warmup    http://127.0.0.1:9117/cron/cloudflare/Warmup 300
0 * * * *        /opt/crabindex/Data/run-job.sh rutracker-parse      http://127.0.0.1:9117/cron/rutracker/parse 3600
```

`/cron/cloudflare/Warmup` по умолчанию открывает поиск Rutracker; другой адрес можно передать параметром `?url=…`. Ответ:

```json
{ "ok": true, "host": "rutracker.org", "length": 123456, "tookSeconds": 12.3 }
```

При успехе хост помечается как защищённый, при неудаче - пометка снимается.

## Проверка

```bash
curl -s http://127.0.0.1:8191/                     # FlareSolverr
curl -s http://127.0.0.1:8192/health               # cffetch
curl -s http://127.0.0.1:9117/cron/cloudflare/Warmup
```

Типичные проблемы - в разделе [Решение проблем](../operations/troubleshooting.md).
