# FlareSolverr и cffetch

Часть трекеров (в первую очередь Rutracker и Kinozal) закрыта проверкой Cloudflare. Обычный HTTP-клиент её не проходит: Cloudflare проверяет браузерный TLS-отпечаток и выдаёт cookie `cf_clearance` только настоящему браузеру. CrabIndex решает это двумя внешними сервисами:

| Сервис | Роль | Порт по умолчанию |
| --- | --- | --- |
| [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) | Запускает Chromium, **решает** challenge и отдаёт cookie и User-Agent | `8191` (второй экземпляр - `8193`) |
| cffetch (`ghcr.io/jacred-fdb/cffetch`) | Быстро **скачивает** страницы с TLS-отпечатком Chrome (curl_cffi), используя полученную cookie | `8192` |

Скачивание через cffetch занимает доли секунды против 0,5-1,2 секунды у браузерного перехода, а FlareSolverr нужен только для получения и обновления cookie.

:::danger[FlareSolverr требователен к ресурсам]
FlareSolverr запускает настоящий браузер Chrome. Решение одной проверки Cloudflare может занять несколько ядер процессора на десятки секунд, а каждая сессия браузера держит 300-600 МБ памяти (во время проверки - до 1-1,5 ГБ). Без ограничений он способен загрузить слабый сервер целиком: сервис перестанет отвечать, а ядро начнёт завершать процессы из-за нехватки памяти.

Ставьте FlareSolverr, только если вам действительно нужно обходить Cloudflare (Rutracker, Kinozal и другие закрытые трекеры), и всегда с лимитами:

- сервер не слабее описанного в разделе [Требования к серверу](#требования-к-серверу);
- лимит контейнера по ресурсам сервера: от `--cpus 1 --memory 1536m` на маленьком VPS до `--cpus 2 --memory 4g` на сервере от 12 ГБ (установщик подбирает их сам, см. [Установка на сервер](#установка-на-сервер));
- `--shm-size 512m`: со стандартными 64 МБ `/dev/shm` Chrome зависает при запуске;
- `DISABLE_MEDIA=true`: браузер не грузит картинки и видео.

Каждый защищённый трекер держит в браузере свою вкладку (`sessionIdleMinutes` минут после последнего запроса), поэтому память растёт с числом таких трекеров. Во время проверки Cloudflare вкладка может резко вырасти: на практике вкладка одного сайта разрослась до 1,2 ГБ, и при лимите 2,5 ГБ и 4 открытых сессиях трекеров браузер завершил её из-за нехватки памяти. С лимитом 4 ГБ проблема ушла. Если в журнале CrabIndex появляется `FlareSolverr отказал: ... tab crashed`, контейнеру не хватает памяти: поднимите лимит (`FLARESOLVERR_MEMORY=4g` для 3-4 трекеров за Cloudflare) или уменьшите `sessionIdleMinutes`. На работающем контейнере лимит меняется без перезапуска: `docker update --memory 4g --memory-swap 4g flaresolverr`.

Если сервер слабый или на нём работают другие тяжёлые сервисы, лучше не ставить FlareSolverr вовсе. Трекеры без Cloudflare продолжат работать, а базу закрытых трекеров можно получать синхронизацией с другого сервера (`syncapi`).
:::

## Требования к серверу

Цифры ниже для CrabIndex вместе с FlareSolverr и cffetch на одной машине, с полной базой (около 1,3 млн файлов, 6 ГБ).

| | Процессор | Память | Диск | Сколько трекеров за Cloudflare |
| --- | --- | --- | --- | --- |
| Минимум | 2 ядра | 4 ГБ | 20 ГБ SSD | 1-2 (Rutracker, Kinozal) |
| Рекомендуется | 4 ядра | 8 ГБ | 40 ГБ SSD/NVMe | 3-4 |
| С запасом | 6+ ядер | 16 ГБ | 60 ГБ NVMe | все, плюс второй экземпляр `crawlUrl` |

Сколько занимают сервисы в работе:

| Сервис | Процессор | Память |
| --- | --- | --- |
| CrabIndex | 0,5-1 ядро при парсинге, до 2 при пересчёте статистики | 1,5-2,5 ГБ |
| FlareSolverr | до 1,5-2 ядер на 10-60 с при решении проверки, в покое почти 0 | 400 МБ на старте плюс 300-600 МБ на каждый трекер за Cloudflare, вкладка во время проверки - до 1-1,5 ГБ |
| cffetch | почти 0 | 30-60 МБ |

Под лимит FlareSolverr по памяти: 1,5 ГБ хватает на 1-2 трекера за Cloudflare, для 3-4 нужно 3-4 ГБ. 2,5 ГБ для 4 трекеров на практике мало: одна вкладка во время проверки занимает до 1,2 ГБ.

:::warning[Когда FlareSolverr лучше не ставить]
- **Меньше 2 ядер или 4 ГБ памяти.** Браузер и CrabIndex будут бороться за ресурсы, парсинг станет медленнее, а при нехватке памяти ядро начнёт завершать процессы, в том числе сам CrabIndex.
- **На сервере уже работают тяжёлые сервисы**, особенно со своим браузером (медиа-серверы, другие агрегаторы). Две копии Chrome на одной машине легко занимают весь процессор.
- **Дешёвый VPS с «общими» ядрами**, где процессор отдают другим клиентам (в `top` видна высокая доля `st`). Проверка Cloudflare на таком сервере решается минутами и упирается в таймауты.
- **Медленный диск (HDD или сетевой).** База состоит из сотен тысяч мелких файлов, и вместе с браузером диск становится узким местом.

В этих случаях ставьте CrabIndex без обхода (`--no-flaresolverr`). Трекеры без Cloudflare продолжат парситься, а данные закрытых трекеров можно получать синхронизацией с сервера, где FlareSolverr есть (`syncapi`).
:::

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
  crawlUrl: ""
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
  proxy: ""
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

## Установка на сервер

Установщик ставит FlareSolverr и cffetch в Docker сам: при установке он спрашивает, нужен ли обход Cloudflare, либо можно передать флаг. Спрашивает он только в режиме собственного парсинга (`--db parse`, см. [Источник базы](../installation.md#db-source)): при синхронизации FlareSolverr не нужен и ставится только по явному `--flaresolverr`.

```bash
sudo bash install.sh --flaresolverr      # поставить (Docker установится автоматически)
sudo bash install.sh --no-flaresolverr   # не ставить и не спрашивать
```

С флагом `--yes` и без `--flaresolverr` обход не ставится, а в `init.yaml` выключаются `flaresolverr.enable` и `cffetch.enable`. Контейнеры слушают только `127.0.0.1` и запускаются с лимитами. Лимиты FlareSolverr установщик подбирает по серверу (память - по `MemTotal` из `/proc/meminfo`, процессор - по `nproc`):

| Память сервера | `FLARESOLVERR_MEMORY` | Ядер | `FLARESOLVERR_CPUS` |
| --- | --- | --- | --- |
| меньше 6 ГБ | `1536m` | 1-2 | `1` |
| 6-12 ГБ | `3g` | 3-5 | `1.5` |
| 12 ГБ и больше | `4g` | 6 и больше | `2` |

Переменные окружения задают лимиты явно и имеют приоритет:

| Переменная | По умолчанию |
| --- | --- |
| `FLARESOLVERR_CPUS` | по числу ядер: `1`, `1.5` или `2` |
| `FLARESOLVERR_MEMORY` | по памяти: `1536m`, `3g` или `4g` |
| `CFFETCH_CPUS` | `0.5` |
| `CFFETCH_MEMORY` | `256m` |

```bash
sudo FLARESOLVERR_CPUS=2 FLARESOLVERR_MEMORY=4g bash install.sh --update --flaresolverr
```

Обычный `--update` уже работающие контейнеры не трогает. Чтобы пересоздать их с новыми лимитами (подобранными заново или заданными переменными), запустите `--update --flaresolverr`.

Вручную это те же две команды:

```bash
docker run -d --name flaresolverr --restart unless-stopped -p 127.0.0.1:8191:8191 \
  -e LOG_LEVEL=info -e DISABLE_MEDIA=true \
  --cpus 2 --memory 4g --shm-size 512m ghcr.io/flaresolverr/flaresolverr:latest

docker run -d --name cffetch --restart unless-stopped --network host \
  --cpus 0.5 --memory 256m ghcr.io/jacred-fdb/cffetch:latest
```

Значения `--cpus` и `--memory` в примере - для сервера от 6 ядер и 12 ГБ памяти; на меньшем сервере возьмите их из таблицы выше.

cffetch внутри контейнера слушает `127.0.0.1:8192`, поэтому ему нужна сеть хоста (`--network host`). `--uninstall` удаляет оба контейнера, если их создал установщик.

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

## Раздел FlareSolverr в админ-панели

В админ-панели есть отдельный раздел **FlareSolverr**. Он обновляется каждые 10 секунд и показывает:

- доступен ли FlareSolverr, его версию и число открытых в браузере сессий;
- режим: работает, приостановлен из панели или выключен в настройках;
- запросы через браузер (успешно / всего, ошибки) и через быстрый путь cffetch;
- таблицу сайтов за Cloudflare: запросы, ошибки с процентом, причины ошибок, среднее время запроса, сессии (создано / пересоздано / закрыто по простою), быстрый путь, последнюю ошибку;
- журнал последних ошибок и список открытых сессий браузера;
- текущие настройки и защищённые хосты.

Причины ошибок:

| Причина | Что значит | Что делать |
| --- | --- | --- |
| Вкладка упала (`tabCrashed`) | Chrome убит по лимиту памяти контейнера | Поднять лимит памяти: `docker update --memory 4g --memory-swap 4g flaresolverr` |
| Таймаут браузера (`browserTimeout`) | Браузер не успел пройти проверку | Поднять лимит CPU (`docker update --cpus 2 flaresolverr`) или убрать часть трекеров за Cloudflare |
| Проверка не пройдена (`challengeFailed`) | Cloudflare не пропустил браузер | Часто это IP дата-центра: см. [WARP](#vps-с-ip-дата-центра-warp) |
| Ошибка сессии (`sessionError`) | Сессия браузера потеряна | Обычно проходит само: CrabIndex пересоздаёт сессию |
| Нет связи (`unreachable`) | FlareSolverr не отвечает | `docker ps`, `docker logs flaresolverr` |
| Страница не подошла (`pageFailed`) | Ответ не 200 или заглушка вместо страницы | Проверить сайт вручную, зеркало, логин |

Когда ошибок много, раздел сам подсказывает, что поднять, и даёт готовую команду. Кнопки:

- **Закрыть сессии** - закрыть все сессии браузера и освободить память. Следующий запрос к трекеру за Cloudflare заново пройдёт проверку. У каждой строки таблицы есть такая же кнопка для одного сайта.
- **Приостановить** - CrabIndex перестаёт обращаться к браузеру и закрывает все его сессии, пока вы не нажмёте **Включить** или не перезапустите службу. Конфиг не меняется. Для постоянного выключения используйте `flaresolverr.enable: false` в настройках.
- **Сбросить статистику** - обнулить счётчики и журнал ошибок.

Лимиты процессора и памяти принадлежат контейнеру FlareSolverr, поэтому меняются командой `docker update` на сервере (без перезапуска) или переустановкой с `FLARESOLVERR_CPUS` / `FLARESOLVERR_MEMORY`. Статистика хранится в памяти и обнуляется при перезапуске CrabIndex. API раздела описан в [Cron API](../api-reference/cron.md#cloudflare-состояние-и-управление).

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
