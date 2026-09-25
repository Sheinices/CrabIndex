# Установка

CrabIndex можно установить несколькими способами:

| Способ | Что нужно | Когда выбирать |
| --- | --- | --- |
| **Установщик `scripts/install.sh`** | Linux с systemd, root, комплект `make dist` (или архив с ним) | Основной способ для сервера: пользователь, служба, crontab и админ-панель настраиваются одной командой |
| **Docker** | Docker | Всё собирается внутри образа, на хосте не нужны ни Rust, ни Node.js. См. [Быстрый старт](quickstart.md) и [Docker](deployment/docker.md) |
| **Вручную** | Бинарник `crabindex` + `wwwroot/` + шаблоны `Data/` | Другие ОС (macOS, [Windows](deployment/windows.md)), система без systemd, пробный запуск из репозитория |

## Рабочий каталог

Приложение работает **относительно текущего каталога** процесса:

```text
/opt/crabindex/            ← рабочий каталог (WorkingDirectory)
├── crabindex              ← бинарник
├── init.yaml              ← runtime-конфиг (или init.conf)
├── wwwroot/               ← сайт, admin/ (админ-панель), docs/, openapi.yaml
└── Data/
    ├── fdb/               ← шарды FileDB
    ├── masterDb.bz        ← индекс FileDB
    ├── temp/              ← состояние задач, stats.json, checkpoints, admin.secret
    ├── log/               ← файловые логи
    ├── tracks/            ← данные модуля Tracks
    ├── example.yaml       ← полный пример конфига (шаблон)
    ├── crontab            ← расписание парсеров
    └── run-job.sh         ← обёртка для cron
```

Каталоги `Data/fdb`, `Data/temp`, `Data/log` и `Data/tracks` создаются автоматически при старте. `Data/example.yaml` - только шаблон: сервер читает `init.yaml` из рабочего каталога, а не из `Data/`.

## Linux: установщик

`scripts/install.sh` ставит CrabIndex как службу systemd:

- проверяет систему и сам ставит недостающие пакеты (см. [Зависимости](#зависимости));
- при необходимости собирает CrabIndex из исходников (Rust и Node.js тоже ставит сам);
- копирует бинарник, `wwwroot/` и шаблоны `Data/` в `/opt/crabindex`;
- создаёт системного пользователя `crabindex` без shell;
- создаёт `init.yaml` из `Data/example.yaml` (права `600`), если конфига ещё нет;
- спрашивает путь админ-панели и генерирует токен входа и пароль (`devkey`);
- устанавливает unit `/etc/systemd/system/crabindex.service` и crontab пользователя `crabindex` из `Data/crontab`;
- включает и запускает службу и печатает адрес админ-панели и пароль.

### Из готового релиза

```bash
curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash
```

Установщик проверит пакеты, скачает последнюю сборку с [GitHub Releases](https://github.com/sheinices/crabindex/releases) под архитектуру сервера (`x86_64` или `arm64`) и установит её. Конкретная версия: `... | sudo bash -s -- --version v1.0.0`. Сборки статические (musl), от версии дистрибутива не зависят.

### Из исходников

```bash
git clone https://github.com/sheinices/crabindex.git
cd crabindex
sudo scripts/install.sh
```

Готового комплекта в репозитории нет, поэтому установщик соберёт его сам: поставит `gcc`, `pkg-config`, `git`, Rust (через rustup) и Node.js 22, соберёт сервер, сайт, админ-панель и документацию и установит. Сборка занимает 10-15 минут. Если нужен git, а его ещё нет: `apt install git` (Debian/Ubuntu) или `dnf install git`.

### 1. Или соберите комплект заранее

```bash
make dist
```

`make dist` собирает сайт, админ-панель, документацию и release-бинарник и складывает всё в `dist/`: `crabindex`, `wwwroot/`, шаблоны `Data/example.yaml`, `Data/example.conf`, `Data/crontab`, `Data/run-job.sh` и сам установщик `dist/install.sh`. Для сборки нужны Rust и Node.js, см. [Сборка из исходников](#сборка-из-исходников). Если на сервере мало памяти, соберите комплект на другой машине той же архитектуры и скопируйте каталог `dist/` (или его архив).

### 2. Запустите установщик

```bash
sudo dist/install.sh
```

Установщик спросит путь админ-панели:

```text
Путь админ-панели:
  1) стандартный /admin
  2) своё название
Выберите [1]:
```

Своё название - один сегмент из строчных латинских букв, цифр, `-` и `_`, длиной 2-32 символа (например, `/my-panel`). Служебные пути (`/api`, `/cron`, `/stats`, `/docs` и другие) заняты.

В конце установщик выведет данные для входа:

```text
════════════════════════════════════════════════════════════
 Админ-панель: http://192.168.1.10:9117/my-panel?Z0mt0N7r2hoUM2TuOk
 Пароль (devkey): 9fQ2…
════════════════════════════════════════════════════════════
 Сохраните эти данные. Повторно: cd /opt/crabindex && sudo -u crabindex ./crabindex admin
```

Откройте адрес в браузере и введите пароль - см. [Админ-панель](admin.md).

### Параметры установщика

| Параметр | Что делает |
| --- | --- |
| `--version vX.Y.Z` | Скачать эту версию с GitHub Releases (без флага, если рядом нет ни комплекта, ни исходников, скачивается последняя) |
| `--from-source [DIR]` | Собрать из исходников (по умолчанию - репозиторий, в котором лежит скрипт) и установить. Недостающие инструменты сборки ставятся сами |
| `--check` | Только проверить систему: systemd, пакеты, архитектуру, место на диске, память, порт 9117. Ничего не меняет |
| `--no-deps` | Не устанавливать системные пакеты (только сообщить, чего не хватает) |
| `--bundle ПУТЬ\|URL` | Откуда брать файлы: каталог `make dist` или архив `.tar.gz` / `.tar.xz` (путь или URL, для URL нужен `curl`). По умолчанию - каталог самого скрипта, если в нём есть `crabindex` (так работает `dist/install.sh`), иначе `../dist` относительно скрипта |
| `--admin-path /x` | Путь админ-панели без вопроса |
| `--yes`, `-y` | Не задавать вопросов: путь `/admin`, если не указан `--admin-path` |
| `--update` | Обновить существующую установку: конфиг и данные сохраняются |
| `--uninstall` | Удалить службу, crontab, бинарник и `wwwroot/`. Данные и конфиг остаются |
| `--purge` | Вместе с `--uninstall`: удалить весь каталог установки (базу, конфиг, логи) и пользователя. Спрашивает подтверждение (или `--yes`) |
| `-h`, `--help` | Справка |

Переменные окружения `INSTALL_DIR` (по умолчанию `/opt/crabindex`) и `SERVICE_USER` (по умолчанию `crabindex`) меняют каталог и пользователя. При другом каталоге пути в crontab переписываются автоматически.

```bash
sudo scripts/install.sh --bundle crabindex.tar.gz          # из архива
sudo dist/install.sh --admin-path /panel --yes              # без вопросов
sudo INSTALL_DIR=/srv/crabindex dist/install.sh             # другой каталог
```

:::note[Примечание]
Если в каталоге установки уже есть `init.yaml`, установщик сохраняет его: заданные `admin.token` и `devkey` не трогает, дописывает только недостающие. Если вместо `init.yaml` используется `init.conf` (JSON), токен и `devkey` сгенерирует сам сервер при первом запуске, а установщик покажет их командой `crabindex admin`.
:::

Если CrabIndex уже установлен в каталоге, обычный запуск установщика сам переходит в режим обновления.

### Зависимости

Перед установкой скрипт показывает сводку и ставит недостающее через `apt`, `dnf`, `yum`, `zypper`, `pacman` или `apk`:

| Что | Зачем |
| --- | --- |
| `curl`, `ca-certificates` | cron-задания (`Data/run-job.sh`), загрузка комплекта по URL |
| `tar`, `xz`, `gzip` | распаковка архивов комплекта |
| `rsync` | копирование комплектов и резервные копии |
| `cron` | расписание парсеров; служба включается автоматически |
| `flock` (`util-linux`) | защита cron-заданий от наложения |
| `ip` (`iproute2`) | определение адреса сервера для ссылки на админ-панель |

Для сборки из исходников дополнительно: компилятор C и `make` (`build-essential`), `pkg-config`, `git`, Rust stable (rustup) и Node.js 20+ (на Debian/Ubuntu и RHEL-подобных - из репозитория NodeSource).

```text
$ sudo scripts/install.sh --check
Проверка системы:
  ✓ systemd
  ✓ менеджер пакетов: apt-get
  ✓ curl
  ✗ rsync - будет установлен (rsync)
  ✗ crontab - будет установлен (cron)
  ✓ flock
  ✓ архитектура x86_64
  ✓ свободно на диске: 65 ГБ
  ✓ память: 7940 МБ
  ✓ порт 9117 свободен
```

### Проверка

```bash
systemctl status crabindex
curl -f http://127.0.0.1:9117/health          # {"status":"OK"}
journalctl -u crabindex -f
```

Детали unit-файла, журналов и резервного копирования - на странице [Linux](deployment/linux.md).

## Docker

```bash
docker build -t crabindex .
docker run -d --name crabindex --restart unless-stopped \
  -p 9117:9117 \
  -v crabindex-config:/app/config \
  -v crabindex-data:/app/Data \
  crabindex
docker logs crabindex 2>&1 | grep 'admin:'     # адрес админ-панели и devkey
```

Путь админ-панели при первом запуске задаётся переменной `CRABINDEX_ADMIN_PATH`. Полный стек с FlareSolverr, WARP и cffetch - на странице [Docker](deployment/docker.md).

## Сборка из исходников

### Требования

- **Rust** stable через [rustup](https://rustup.rs);
- **Node.js 22+** и npm - для сайта (`web/`), админ-панели (`admin/`) и документации (`docs/`);
- `git`, `make`, `bash`.

### Сборка

```bash
git clone https://github.com/sheinices/crabindex.git
cd crabindex

make web        # сайт → wwwroot/, админ-панель → wwwroot/admin/, документация → wwwroot/docs/
make release    # target/release/crabindex
```

Для кросс-сборки передайте целевую платформу: `make release TARGET=aarch64-unknown-linux-gnu` (toolchain и линкер для неё нужно установить отдельно).

Подробнее о целях `make` и структуре проекта - в разделе [Сборка](development/building.md).

### Пробный запуск из репозитория

```bash
cp Data/example.yaml init.yaml   # отредактируйте под себя
./target/release/crabindex
```

После запуска:

- сайт - `http://127.0.0.1:9117/`;
- админ-панель - адрес и `devkey` выводятся в лог при первом запуске (`admin: http://…/admin?…`, `admin: devkey: …`), повторно - `./target/release/crabindex admin`;
- проверка - `curl http://127.0.0.1:9117/health` → `{"status":"OK"}`;
- версия сборки - `http://127.0.0.1:9117/version`.

## Установка вручную (systemd)

Если установщик не подходит, те же шаги можно выполнить руками:

```bash
make dist
sudo useradd --system --home /opt/crabindex --shell /usr/sbin/nologin crabindex
sudo mkdir -p /opt/crabindex
sudo cp -a dist/. /opt/crabindex/
sudo cp /opt/crabindex/Data/example.yaml /opt/crabindex/init.yaml
sudo chown -R crabindex:crabindex /opt/crabindex
sudo chmod 600 /opt/crabindex/init.yaml
```

Создайте unit-файл (пример - на странице [Linux](deployment/linux.md)), затем:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now crabindex
sudo crontab -u crabindex /opt/crabindex/Data/crontab
cd /opt/crabindex && sudo -u crabindex ./crabindex admin   # адрес админ-панели и devkey
```

:::note[Примечание]
`WorkingDirectory` в unit-файле обязателен: без него процесс не найдёт `init.yaml`, `Data/` и `wwwroot/`.
:::

Расписание парсеров не обязательно, если база наполняется синхронизацией (`syncapi`). Подробнее - [Cron](deployment/cron.md) и [Синхронизация](concepts/sync.md).

## Подготовка базы

После первого запуска FileDB пуста. Заполните её одним из способов:

- **синхронизация** - задайте `syncapi` (в шаблоне уже указан `https://sync.crab.rip`), и sync-воркер загрузит готовую базу;
- **собственный парсинг** - установите `Data/crontab` (установщик делает это сам);
- **перенос** - скопируйте `Data/fdb/` и `Data/masterDb.bz` с другого инстанса (при остановленном сервере).

## Настройка

Удобнее всего менять настройки в [админ-панели](admin.md) (раздел **Настройки**): форма, текстовый режим YAML/JSON, проверка и diff перед сохранением. Можно править и файл:

```bash
sudoedit /opt/crabindex/init.yaml
```

Перезапуск не нужен: сервер проверяет файл каждые 10 секунд и применяет изменения. См. [Обзор конфигурации](configuration/overview.md).

## Обновление

```bash
cd crabindex && git pull
make dist
sudo dist/install.sh --update
```

Установщик останавливает службу, заменяет бинарник, `wwwroot/` и шаблоны в `Data/` (`example.*`, `crontab`, `run-job.sh`), переустанавливает crontab и запускает службу снова. `init.yaml`, база и логи не меняются. При остановке по SIGTERM сервер сам завершает фоновые задачи и сбрасывает на диск кеш FileDB и `masterDb`.

:::warning[Внимание]
Перед обновлением сделайте резервную копию `/opt/crabindex/Data`. Если вы меняли `Data/crontab` или crontab пользователя `crabindex`, сохраните изменения: при обновлении они заменяются версией из комплекта.
:::

## Удаление

```bash
sudo dist/install.sh --uninstall           # служба, crontab, бинарник, wwwroot; данные остаются
sudo dist/install.sh --uninstall --purge   # всё, включая FileDB, конфиг и пользователя
```
