# Docker

`Dockerfile` в корне репозитория собирает образ в несколько этапов: сайт (`web/`), админ-панель (`admin/`) и документация (`docs/`) на Node.js, бинарник `crabindex` на Rust и минимальный runtime на `debian:bookworm-slim`. Rust и Node.js на хосте не нужны.

:::tip[Совет]
Для обычного Linux-сервера без Docker есть установщик `scripts/install.sh`, см. [Установка](../installation.md).
:::

## Параметры образа

| Параметр | Значение |
| --- | --- |
| Сборка | `docker build -t crabindex .` или `make docker` |
| Порт | `9117/tcp` |
| Тома | `/app/config` (конфигурация), `/app/Data` (FileDB, логи, Tracks) |
| Рабочий каталог | `/app` |
| Пользователь | `crabindex:crabindex`, UID/GID `1000:1000` (не root) |
| Entrypoint | `dumb-init -- /entrypoint.sh`, команда `./crabindex` |
| Healthcheck | `curl -f http://127.0.0.1:9117/health` каждые 30 с |

Переменные окружения:

| Переменная | По умолчанию | Описание |
| --- | --- | --- |
| `TZ` | `UTC` | Часовой пояс (влияет на даты в логах и имена дневных копий `masterDb`) |
| `UMASK` | `0027` | Маска прав создаваемых файлов |
| `CONFIG_FILE_MODE` | `600` | Права на копии `init.yaml` / `init.conf` |
| `CRABINDEX_ADMIN_PATH` | - | Путь [админ-панели](../admin.md) (например, `/my-panel`), который записывается в конфиг при первой генерации токена и `devkey`. На уже настроенный конфиг не влияет |

Аргументы сборки `CRABINDEX_VERSION`, `CRABINDEX_GIT_SHA`, `CRABINDEX_GIT_BRANCH` задают версию, если сборка идёт без каталога `.git`:

```bash
docker build --build-arg CRABINDEX_VERSION=0.1.0 -t crabindex .
```

## Как выбирается конфигурация

При каждом старте контейнера entrypoint ищет конфигурацию в таком порядке:

1. `/app/config/init.yaml`
2. `/app/config/init.conf`
3. `/app/Data/init.yaml`, затем `/app/Data/init.conf` - только для первичной инициализации
4. `/app/defaults/init.yaml` (копия `Data/example.yaml`), затем `/app/defaults/init.conf`

Найденный файл сохраняется в томе как `/app/config/init.*`, а `/app/init.*` (рабочий каталог процесса) становится символьной ссылкой на него. Файл другого формата удаляется: `init.yaml` всегда побеждает `init.conf`.

:::tip[Совет]
Админ-панель пишет прямо в файл тома, поэтому изменения переживают перезапуск и пересоздание контейнера. Ручные правки `/app/config/init.yaml` на хосте тоже подхватываются без перезапуска: сервер перечитывает конфиг каждые 10 секунд.
:::

## Минимальный запуск

```yaml
# docker-compose.yml
services:
  crabindex:
    image: crabindex:latest
    build:
      context: .
    container_name: crabindex
    restart: unless-stopped
    ports:
      - "9117:9117"
    volumes:
      - ./config:/app/config
      - ./data:/app/Data
    environment:
      - TZ=Europe/Moscow
```

```bash
docker compose up -d --build
docker compose logs -f crabindex
curl -f http://localhost:9117/health
```

## Вход в админ-панель

При первом запуске сервер генерирует `admin.token` и `devkey`, записывает их в `/app/config/init.yaml` (в томе) и выводит в лог:

```bash
docker logs crabindex 2>&1 | grep 'admin:'
# admin: http://<host>:9117/admin?Z0mt0N7r2hoUM2TuOk
# admin: devkey: 9fQ2…
```

Внутри контейнера адрес хоста неизвестен, поэтому в логе стоит `<host>`: подставьте адрес машины или домен обратного прокси. Повторно вывести адрес и ключ:

```bash
docker exec crabindex ./crabindex admin
```

Чтобы панель сразу получила свой путь, задайте переменную до первого запуска:

```yaml
    environment:
      - TZ=Europe/Moscow
      - CRABINDEX_ADMIN_PATH=/my-panel
```

Подробнее о входе, смене токена и ключа - [Админ-панель](../admin.md).

При bind-mount каталоги `./config` и `./data` должны быть доступны на запись пользователю `1000:1000`:

```bash
mkdir -p config data && sudo chown -R 1000:1000 config data
```

## Полный стек: FlareSolverr, cffetch, WARP

Файл [docker-compose.example.yml](https://github.com/sheinices/crabindex/blob/main/docker-compose.example.yml) поднимает всё, что нужно для трекеров за Cloudflare на VPS:

| Сервис | Образ | Сеть | Назначение |
| --- | --- | --- | --- |
| `crabindex` | собирается из `Dockerfile` | bridge, порт `9117` | Сервер |
| `warp` | `caomingjun/warp` | bridge, `127.0.0.1:20001` → SOCKS5 | Выход в интернет через Cloudflare WARP |
| `flaresolverr` | `ghcr.io/flaresolverr/flaresolverr` | host, `8191` | Решение challenge для обычных запросов |
| `flaresolverr-crawl` | `ghcr.io/flaresolverr/flaresolverr` | host, `8193` | Отдельный браузер для фоновых обходов |
| `cffetch` | `ghcr.io/jacred-fdb/cffetch` | host, `8192` | Быстрое скачивание с TLS Chrome |

```bash
cp docker-compose.example.yml docker-compose.yml
docker compose up -d --build
```

FlareSolverr и cffetch работают в `network_mode: host`, чтобы выходить через один и тот же SOCKS WARP: `cf_clearance` привязана к IP. Поэтому для CrabIndex в bridge-сети они доступны через `host.docker.internal`:

```yaml
# /app/config/init.yaml
flaresolverr:
  enable: true
  url: http://host.docker.internal:8191/v1
  crawlUrl: http://host.docker.internal:8193/v1

cffetch:
  enable: true
  url: http://host.docker.internal:8192/fetch
  proxy: socks5://127.0.0.1:20001
```

Сервису `crabindex` нужна строка `extra_hosts: ["host.docker.internal:host-gateway"]` (в примере она есть).

:::note[Примечание]
`cffetch.proxy` из конфигурации CrabIndex передаётся в запросы к cffetch. Сам контейнер cffetch в примере уже настроен на тот же SOCKS переменной `CFFETCH_PROXY`.
:::

Том `warp-data` сохраняет регистрацию WARP между перезапусками. Без него WARP каждый раз получает новый IP, и проверки Cloudflare случаются чаще. Подробнее - [FlareSolverr и cffetch](../configuration/flaresolverr.md).

## Cron для контейнера

В образе нет планировщика. Парсеры вызываются по HTTP с хоста (или из отдельного контейнера) по расписанию `Data/crontab`. Скрипт `run-job.sh` и `crontab` лежат в репозитории и в томе `/app/Data`. Пример установки на хосте:

```bash
sudo mkdir -p /opt/crabindex/Data
sudo cp Data/run-job.sh Data/crontab /opt/crabindex/Data/
crontab /opt/crabindex/Data/crontab
```

Задания обращаются к `http://127.0.0.1:9117`. Запросы с хоста к опубликованному порту приходят в контейнер с адреса шлюза Docker (частная сеть), поэтому `devkey` для них не требуется, если между ними нет обратного прокси. Если сомневаетесь, добавьте `?devkey=…` к адресам (значение - из `/app/config/init.yaml`). Подробнее - [Cron](cron.md).

## Обновление

```bash
git pull
docker compose up -d --build
```

Данные в томах `/app/Data` и `/app/config` сохраняются. Перед остановкой контейнер получает SIGTERM (через `dumb-init`), и сервер сбрасывает FileDB на диск.

:::warning[Внимание]
Перед обновлением сделайте резервную копию тома `/app/Data`.
:::

## Проверка

```bash
curl http://localhost:9117/health                    # {"status":"OK"}
curl http://localhost:9117/version                   # версия, git SHA, дата сборки
curl http://localhost:9117/health/background-jobs    # активные фоновые задачи
docker inspect --format '{{.State.Health.Status}}' crabindex
```
