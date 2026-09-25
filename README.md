<p align="center">
  <img src="assets/brand/background.png" alt="CrabIndex" width="640">
</p>

# CrabIndex

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Агрегатор торрент-трекеров на Rust для [Prisma](https://t.me/prisma_party), Lampa, Sonarr, Radarr и Prowlarr с API в формате Jackett. Хранит данные в файловой БД (FileDB), умеет синхронизироваться с удалённой базой и самостоятельно парсить трекеры по cron. Один статически собранный бинарник `crabindex` + веб-интерфейс в `wwwroot/`.

## Основные возможности

- 🔍 **Агрегация торрентов** с множества трекеров в единый API
- 📺 **Prisma** - основной клиент: подключается как Jackett-источник, поиск по карточке фильма ([инструкция](docs/docs/clients/overview.md#prisma))
- 📦 **Файловая БД (FileDB)** - шардированное хранилище в `Data/fdb` с `masterDb` и кешем в памяти
- 🔄 **Синхронизация** с удалённым сервером (`syncapi`) или самостоятельный парсинг
- 🎯 **API Jackett** - совместимый формат выдачи
- 📡 **Torznab XML** - встроенный Torznab API для Sonarr / Radarr / Prowlarr
- 🌐 **Веб-интерфейс** - поиск, статистика, фоновые задачи и редактор конфигурации
- ⚙️ **Админ-панель** - настройки (форма, YAML/JSON, валидация, diff, горячая перезагрузка), задачи, логи и обслуживание по закрытому адресу `{admin.path}?{admin.token}` с паролем `devkey`
- 📚 **Документация** - встроена в сервер: `http://<хост>:9117/docs/` (исходники в [`docs/`](docs/), Docusaurus)
- 📖 **OpenAPI / Swagger** - `/openapi.yaml`, `/swagger/v1/swagger.json`, интерактивная документация на `/swagger`
- 🗂️ **25 трекеров** - парсинг и sync
- 🔐 **Прокси** (HTTP/SOCKS, Tor для .onion), FlareSolverr + cffetch для хостов за Cloudflare
- 📊 **Статистика** по трекерам и торрентам
- 🎵 **Модуль tracks** - сбор метаданных аудио/видео дорожек через TorrServer / ffprobe (опционально)
- 🦀 **Rust / tokio / axum** - асинхронный сервер, низкое потребление памяти, быстрый старт
- 🐳 **Docker** - готовый `Dockerfile` и пример `docker-compose`

---

## Требования

- **Rust** stable (через [rustup](https://rustup.rs)) - для сборки из исходников
- **Node.js 22+** и npm - только для сборки веб-интерфейса (`web/`)
- Linux / macOS / Windows; для продакшена рекомендуется Linux (systemd, cron)
- Либо только **Docker** - всё собирается внутри образа

Внешних системных библиотек не требуется: TLS реализован на rustls.

---

## Сборка и запуск

```bash
make web        # Vue SPA → wwwroot/
make release    # target/release/crabindex
cp Data/example.yaml init.yaml   # отредактируйте под себя
./target/release/crabindex
```

Приложение работает относительно текущего каталога: читает `init.yaml` (или `init.conf`), хранит базу в `Data/`, отдаёт статику из `wwwroot/`.

После запуска:

- Веб-интерфейс: **`http://127.0.0.1:9117/`** (поиск), **`/stats`**
- Админ-панель: адрес и пароль выводятся в лог при первом запуске (`admin: http://…/admin?<token>`, `admin: devkey: …`); повторно - `./crabindex admin`
- Проверка: `curl http://127.0.0.1:9117/health` → `{"status":"OK"}`
- Версия сборки: `/version`

Цели `make`:

| Цель | Описание |
| --- | --- |
| `make build` | Debug-сборка |
| `make release` | Release-бинарник (`TARGET=` для кросс-сборки) |
| `make test` | Все тесты workspace |
| `make web` | Сборка веб-интерфейса в `wwwroot/` |
| `make dist` | Готовый бандл в `dist/`: бинарник, `wwwroot/`, шаблоны `Data/` |
| `make docker` | Docker-образ `crabindex` |

Версия, git SHA, ветка и дата сборки вшиваются в бинарник при компиляции (`build.rs`) и печатаются при старте. Без `.git` их можно задать переменными `CRABINDEX_VERSION`, `CRABINDEX_GIT_SHA`, `CRABINDEX_GIT_BRANCH`, `CRABINDEX_BUILD_DATE`.

### Установка как сервис (systemd)

Установщик (Linux + systemd) ставит комплект `make dist` в `/opt/crabindex`, создаёт пользователя
`crabindex`, unit и crontab, спрашивает путь админ-панели и генерирует токен и `devkey`:

```bash
sudo scripts/install.sh                      # из клона репозитория: проверит пакеты, соберёт и установит
sudo scripts/install.sh --check              # только проверить систему
make dist && sudo dist/install.sh            # или: sudo scripts/install.sh --bundle crabindex.tar.gz
sudo dist/install.sh --update                # обновление (конфиг и данные сохраняются)
sudo dist/install.sh --admin-path /panel --yes   # без вопросов
sudo dist/install.sh --uninstall [--purge]
```

Вручную:

```ini
# /etc/systemd/system/crabindex.service
[Unit]
Description=CrabIndex
After=network-online.target

[Service]
WorkingDirectory=/opt/crabindex
ExecStart=/opt/crabindex/crabindex
Restart=always
User=crabindex

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload && systemctl enable --now crabindex
crontab /opt/crabindex/Data/crontab   # полный набор задач парсинга
```

При остановке (SIGTERM / Ctrl+C) сервер завершает фоновые задачи и сбрасывает на диск кеш FileDB и `masterDb`.

---

## Docker

```bash
docker build -t crabindex .
docker run -d --name crabindex -p 9117:9117 \
  -v crabindex-config:/app/config -v crabindex-data:/app/Data crabindex
```

Или `docker compose` на основе [docker-compose.example.yml](docker-compose.example.yml) - вместе с FlareSolverr, WARP и cffetch для трекеров за Cloudflare.

При первом запуске сервер генерирует `admin.token` и `devkey`, записывает их в конфиг (том `/app/config`) и выводит адрес админ-панели в лог (`docker logs crabindex 2>&1 | grep admin:`; путь можно задать через `CRABINDEX_ADMIN_PATH`).

Конфиг при старте контейнера выбирается так: `/app/config/init.yaml` → `/app/config/init.conf` → `/app/Data/init.*` → шаблон по умолчанию (`Data/example.yaml`). Cron-задачи запускаются снаружи контейнера (`Data/crontab`, `Data/run-job.sh`).

---

## Конфигурация

Файл `init.yaml` (приоритетнее) или `init.conf` (JSON) в рабочем каталоге. Полный пример с комментариями - [Data/example.yaml](Data/example.yaml). Указываются только отличающиеся от умолчаний ключи; изменения файла подхватываются автоматически (проверка раз в 10 секунд).

Основное:

| Ключ | Описание |
| --- | --- |
| `listenip`, `listenport` | Адрес и порт (`any` = все интерфейсы, по умолчанию `9117`) |
| `apikey` | Ключ для поисковых API (пусто - без проверки) |
| `devkey` | Пароль админ-панели; ключ для `/dev/`, `/cron/`, `/jsondb` из интернета (генерируется при первом запуске) |
| `admin` | Админ-панель: `enable`, `path` (по умолчанию `/admin`), `token` (генерируется), `sessionHours` |
| `web` | Раздавать веб-интерфейс из `wwwroot/` |
| `syncapi`, `synctrackers` | Синхронизация с удалённым сервером |
| `disable_trackers` | Трекеры, отключённые на этом инстансе |
| `evercache`, `fdbPathLevels` | Кеш и структура FileDB |
| `logging` | Уровни логов по категориям (`tracks`, `sync`, `cron`, `fdb`, `parsers`…) |
| `flaresolverr`, `cffetch`, `proxy`, `globalproxy` | Сеть и обход Cloudflare |
| `search`, `torznab`, `alloha` | Поиск и Torznab |

Настройки можно менять в админ-панели - сохранение атомарное, с валидацией и просмотром diff.

---

## Доступ и безопасность

| Пути | Политика |
| --- | --- |
| `/`, `/stats`, `/health`, `/version`, `/lastupdatedb`, `/api/v1.0/conf`, `/sync/*`, статика, `/swagger`, `/openapi.yaml` | Публично |
| Поиск: `/api/v1.0/torrents`, `/api/v2.0/indexers/*`, `/torznab/api`, `/stats/*` … | `apikey`, если задан |
| `/dev/*`, `/cron/*`, `/jsondb*` | Локальная сеть или `devkey` |
| `{admin.path}/*` (админ-панель и её API) | Токен входа (`{admin.path}?{token}`) + вход по `devkey`; без них - обычный 404 |

- `apikey` передаётся как `?apikey=`, заголовок `X-Api-Key` или `Authorization: Bearer …`.
- `devkey` - заголовок `X-Dev-Key` или `?devkey=`.
- «Локальная сеть» - прямое подключение с loopback / RFC1918 / IPv6 ULA и link-local. Запросы через обратный прокси (заголовки `X-Forwarded-*`, `Forwarded`, `CF-Connecting-IP`, `X-Real-IP`) считаются внешними и требуют `devkey`.
- `X-Forwarded-For` / `X-Forwarded-Proto` учитываются только если прокси работает на том же хосте (loopback).
- Маршрутизация нечувствительна к регистру пути и имён query-параметров.

---

## Обслуживание FileDB

```bash
./crabindex maintain --mode=report   # отчёт о целостности
./crabindex maintain --mode=safe     # безопасные исправления
./crabindex maintain --mode=full     # полное обслуживание
```

Те же операции доступны по HTTP: `/cron/maintenance/Check?mode=report|safe|full`.

---

## Структура проекта

| Крейт | Назначение |
| --- | --- |
| `crates/crabindex` | Бинарник: HTTP-сервер, безопасность, конфиг API, health, Swagger, фоновые задачи |
| `crates/crab-core` | Модели, конфигурация, FileDB, HTTP-клиент, парсинг, логирование |
| `crates/crab-cloudflare` | FlareSolverr / cffetch, прогрев Cloudflare-сессий |
| `crates/crab-trackers-*` | Парсеры трекеров |
| `crates/crab-search` | Поиск, Jackett / Torznab / torrents API |
| `crates/crab-tracks` | Модуль tracks и статистика |
| `crates/crab-ops` | Sync, cron трекеров, обслуживание БД, `maintain` |
| `web/` | Веб-интерфейс (Vue + Vite), `web/public/openapi.yaml` - контракт API |

---

## Лицензия

MIT License. См. файл [LICENSE](LICENSE).
