# Сборка

CrabIndex - cargo workspace на Rust плюс два веб-приложения на React: публичный сайт в `web/` и админ-панель в `admin/`. Основной интерфейс сборки - `Makefile` в корне репозитория (`make help` выводит список целей).

## Требования

- **Rust** stable через [rustup](https://rustup.rs) (edition 2021);
- **Node.js 22+** и npm 10+ - для сайта, админ-панели и документации (Docusaurus); сайту нужен Node.js не ниже 20.19;
- `make`, `bash`, `git`.

Системные библиотеки не нужны: HTTP-клиент использует rustls.

## Структура репозитория

```text
crabindex/
├── Cargo.toml              # workspace: members = ["crates/*"]
├── crates/
│   ├── crabindex/          # бинарник: HTTP-сервер, безопасность, админ-панель (src/admin), Config API, Swagger, воркеры, maintain
│   ├── crab-core/          # конфигурация, модели, FileDB, HTTP-клиент, парсинг, логи, задачи трекеров
│   ├── crab-cloudflare/    # FlareSolverr, cffetch, /cron/cloudflare/Warmup
│   ├── crab-trackers-a/    # rutor, megapeer, torrentby, kinozal, nnmclub
│   ├── crab-trackers-b/    # rutracker, toloka, mazepa, selezen, bitru
│   ├── crab-trackers-c/    # lostfilm, animelayer, anidub, anistar, baibako
│   ├── crab-trackers-d/    # anibelka, aniliberty, anifilm, leproduction, viruseproject, korsars
│   ├── crab-trackers-e/    # ultradox, knaben, rudub, subsplease
│   ├── crab-search/        # Jackett, Torznab, Prowlarr, собственный API, Alloha
│   ├── crab-tracks/        # модуль Tracks и /stats/*
│   └── crab-ops/           # sync, /jsondb/save, обслуживание FileDB, ResumeParseAll, /dev/*, CLI maintain
├── web/                    # сайт (поиск, статистика): React + Vite; web/public/openapi.yaml - контракт API
├── admin/                  # админ-панель: React + Vite → wwwroot/admin/
├── docs/                   # эта документация (Docusaurus)
├── Data/                   # example.yaml, example.conf, crontab, run-job.sh
├── scripts/
│   ├── build-web-ui.sh     # сборка web/ → wwwroot/, admin/ → wwwroot/admin/, docs/ → wwwroot/docs/
│   └── install.sh          # установщик для Linux + systemd
├── Dockerfile, docker-compose.example.yml, entrypoint.sh
└── Makefile
```

Зависимости между крейтами идут в одну сторону: все крейты зависят от `crab-core`, а бинарник `crabindex` собирает их вместе - вызывает `init()` каждого модуля, объединяет их `router()` и запускает фоновые воркеры.

## Цели make

| Цель | Что делает |
| --- | --- |
| `make build` | Debug-сборка сервера (`cargo build -p crabindex`) |
| `make release` | Оптимизированный бинарник `target/release/crabindex`; `TARGET=` для кросс-сборки |
| `make run` | Запуск сервера из корня репозитория (`cargo run -p crabindex`) |
| `make test` | Все тесты Rust (`cargo test --workspace`) |
| `make check` | Проверка типов всего workspace, включая тесты |
| `make fmt` | `cargo fmt --all` |
| `make clippy` | `cargo clippy --workspace --all-targets` |
| `make web` | Сборка сайта в `wwwroot/`, админ-панели в `wwwroot/admin/` и документации в `wwwroot/docs/` |
| `make docs` | Только документация в `wwwroot/docs/` |
| `make dev-web` | Dev-сервер Vite (`web/`), проксирует API на запущенный сервер |
| `make test-web` | Unit-тесты сайта (Vitest) |
| `make dist` | Бандл в `dist/`: бинарник, `wwwroot/`, `Data/example.yaml`, `example.conf`, `crontab`, `run-job.sh` и установщик `install.sh` |
| `make docker` | Docker-образ `crabindex` (`DOCKER_IMAGE=` для другого имени) |
| `make clean` | Удалить `target/`, `wwwroot/`, `dist/`, `web/dist` |
| `make clean-all` | `clean` + `web/node_modules` и `docs/node_modules` |

Для админ-панели отдельных целей `make` нет: пользуйтесь `npm` в каталоге `admin/` (см. ниже).

Кросс-сборка:

```bash
rustup target add aarch64-unknown-linux-gnu
make release TARGET=aarch64-unknown-linux-gnu   # → target/aarch64-unknown-linux-gnu/release/crabindex
```

Для целевой платформы нужен соответствующий линкер (например, `gcc-aarch64-linux-gnu`).

## Версия сборки

`crates/crabindex/build.rs` вшивает в бинарник версию, git SHA, ветку и дату сборки. Версия берётся из git-тега (`v0.1.0` → `0.1.0`; вне тега - `<последний тег>-next+<sha>`; без тегов - `<версия из Cargo.toml>-dev+<sha>`). Версия API в Swagger (`info.version` в `/openapi.yaml`) всегда равна версии из `Cargo.toml` (`[workspace.package] version`). Без каталога `.git` значения можно задать переменными окружения при сборке: `CRABINDEX_VERSION`, `CRABINDEX_GIT_SHA`, `CRABINDEX_GIT_BRANCH`, `CRABINDEX_BUILD_DATE`. Версия печатается при старте и отдаётся на `/version`.

## Тесты

```bash
cargo test --workspace            # или make test
cargo test -p crab-trackers-a     # тесты одного крейта
cargo test -p crab-trackers-a --test rutor
```

Модульные тесты лежат рядом с кодом (`#[cfg(test)]`), интеграционные - в `crates/<крейт>/tests/`. Тесты парсеров работают на сохранённых HTML/JSON-страницах из `tests/fixtures/<трекер>/` и не ходят в сеть.

## Веб-интерфейс и админ-панель

Оба приложения написаны на JavaScript (JSX, без TypeScript):

| | Сайт (`web/`) | Админ-панель (`admin/`) |
| --- | --- | --- |
| Страницы | `/` поиск, `/stats` статистика | Обзор, Трекеры, Задачи, Настройки, Обслуживание, Логи |
| Стек | React 19, react-router, Vite, Tailwind CSS 4, lucide-react | То же плюс CodeMirror (редактор YAML/JSON) |
| Тесты | Vitest + React Testing Library | Vitest + React Testing Library |
| Сборка | `web/dist/` → копируется в `wwwroot/` | сразу в `wwwroot/admin/` |

```bash
make web
```

Скрипт `scripts/build-web-ui.sh`:

1. выполняет `npm ci` и `npm run build` в `web/`;
2. проверяет, что в `web/dist` есть `index.html`, `openapi.yaml` и `sw.js`;
3. **удаляет и создаёт заново** `wwwroot/`, копирует туда `web/dist`;
4. сохраняет `wwwroot/trackers.txt`, если он был;
5. собирает админ-панель (`admin/`) в `wwwroot/admin/`;
6. собирает документацию (`docs/`) в `wwwroot/docs/`.

`wwwroot/` не хранится в git и целиком генерируется сборкой. Сервер отдаёт `wwwroot/admin/` только через шлюз админ-панели, а не как обычную статику.

:::warning[Внимание]
Если собираете `web/` вручную и копируете `web/dist` в `wwwroot/`, не удаляйте `wwwroot/admin/` и `wwwroot/docs/`, либо пересоберите их после этого.
:::

Разработка и проверки сайта:

```bash
cd web
npm ci
npm run dev          # http://localhost:5173, API проксируется на http://127.0.0.1:9117
npm run lint
npm test
npm run build
```

Другой сервер для прокси задаёт переменная `VITE_API_PROXY_TARGET`.

Разработка и проверки админ-панели:

```bash
cd admin
npm ci
npm run dev          # dev-сервер Vite со встроенным mock API (admin/mock/), сервер не нужен
npm run lint
npm test
npm run build        # → ../wwwroot/admin/
```

Сборка админ-панели использует относительные пути к ресурсам (`base: './'`), поэтому работает под любым `admin.path`. Путь панели сервер подставляет в `index.html` мета-тегом `crab-admin-base`.

Контракт API - `web/public/openapi.yaml`. Он же отдаётся сервером на `/openapi.yaml` и используется Swagger UI на `/swagger`. После изменения API обновите этот файл.

## Документация

```bash
make docs                            # → wwwroot/docs/
```

Сервер отдаёт документацию по адресу `http://<host>:9117/docs/`. Подробнее - [Документация](docs-workflow.md).

## Полная сборка с нуля

```bash
make web            # сайт, админ-панель и документация
make release
cp Data/example.yaml init.yaml
./target/release/crabindex
```

При первом запуске сервер сгенерирует `admin.token` и `devkey`, допишет их в `init.yaml` и выведет адрес [админ-панели](../admin.md) в лог.

Сервер работает относительно текущего каталога - запускайте его из корня репозитория (или используйте `make run`).

## Релизы

Сборки выпускает GitHub Actions (`.github/workflows/release.yml`) при публикации релиза на GitHub:

1. Создайте релиз с тегом `vX.Y.Z` («Create a new release» → «Publish release»).
2. Workflow собирает `wwwroot/` (сайт, админ-панель, документация) и сервер под Linux x86_64 и arm64 (musl), macOS arm64 и x86_64, Windows x86_64.
3. К релизу прикрепляются архивы `crabindex-<платформа>.tar.gz` / `.zip` (имена без версии, поэтому ссылка `/releases/latest/download/<имя>` всегда ведёт на последнюю сборку) и `SHA256SUMS`.
4. Публикуется Docker-образ `ghcr.io/sheinices/crabindex:<версия>` и `:latest` для amd64 и arm64 (`Dockerfile.release`, из готовых бинарников).

Версия берётся из тега и видна в баннере, `/version`, админ-панели и Swagger. Пересобрать уже существующий тег можно вручную: Actions → Release → «Run workflow» с указанием тега.

На каждый push в `main` и pull request `.github/workflows/ci.yml` запускает тесты Rust, сайта и админ-панели, сборку документации и shellcheck скриптов.
