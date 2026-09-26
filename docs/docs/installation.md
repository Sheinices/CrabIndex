# Установка

CrabIndex - один бинарник `crabindex` плюс веб-интерфейс `wwwroot/` и шаблоны `Data/`. Готовые сборки для Linux, macOS и Windows и Docker-образ выпускаются с каждым [релизом](https://github.com/sheinices/crabindex/releases/latest).

| Способ | Что нужно | Когда выбирать |
| --- | --- | --- |
| [Linux: установщик](#linux-installer) | Linux с systemd, root | Основной способ для сервера: пакеты, пользователь, служба, crontab и админ-панель настраиваются одной командой |
| [Linux: вручную (systemd)](#linux-manual) | Linux с systemd, root | Если установщик не подходит или хочется контролировать каждый шаг |
| [Docker](#docker) | Docker | Сервер с Docker, NAS, Docker Desktop на macOS и Windows. Полный стек с FlareSolverr, WARP и cffetch |
| [macOS](#macos) | macOS на Apple Silicon или Intel | Домашняя установка, разработка |
| [Windows](#windows) | Windows 10/11 x64 | Домашняя установка |
| [Из исходников](#from-source) | Rust, Node.js | Разработка, своя архитектура, изменения в коде |

После любого способа установки:

- сайт (поиск) - `http://<хост>:9117/`, проверка - `http://<хост>:9117/health` → `{"status":"OK"}`;
- [админ-панель](admin.md) - адрес с токеном и пароль (`devkey`) выводятся при первом запуске, повторно - командой `crabindex admin`;
- конфигурация - файл `init.yaml` в рабочем каталоге (в Docker - `/app/config/init.yaml`), см. [Обзор конфигурации](configuration/overview.md);
- база сразу после установки пуста - см. [Подготовка базы](#prepare-db).

## Рабочий каталог {#workdir}

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

Каталоги `Data/fdb`, `Data/temp`, `Data/log` и `Data/tracks` создаются автоматически при старте. `Data/example.yaml` - только шаблон: сервер читает `init.yaml` из рабочего каталога, а не из `Data/`. Запускайте `crabindex` из каталога, где лежат `init.yaml`, `Data/` и `wwwroot/`.

## Linux: установщик {#linux-installer}

`scripts/install.sh` ставит CrabIndex как службу systemd:

- проверяет систему и сам ставит недостающие пакеты (см. [Зависимости](#dependencies));
- скачивает готовую сборку с GitHub Releases под архитектуру сервера (`x86_64` или `arm64`) или собирает CrabIndex из исходников (Rust и Node.js тоже ставит сам);
- копирует бинарник, `wwwroot/` и шаблоны `Data/` в `/opt/crabindex`;
- создаёт системного пользователя `crabindex` без shell;
- создаёт `init.yaml` из `Data/example.yaml` (права `600`), если конфига ещё нет;
- если порт `9117` занят, предлагает свободный (см. [Занятый порт](#busy-port));
- спрашивает путь админ-панели и генерирует токен входа и пароль (`devkey`);
- по желанию ставит FlareSolverr и cffetch в Docker для трекеров за Cloudflare;
- устанавливает unit `/etc/systemd/system/crabindex.service` и crontab пользователя `crabindex` из `Data/crontab`;
- включает и запускает службу и печатает адрес админ-панели, пароль и порт.

### Шаги

1. Подключитесь к серверу под пользователем с `sudo` (Debian, Ubuntu, RHEL/Alma/Rocky, Fedora, openSUSE, Arch и другие дистрибутивы с systemd).

2. При желании проверьте систему - установщик покажет, чего не хватает, и ничего не изменит:

   ```bash
   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --check
   ```

3. Запустите установщик. Выберите один вариант:

   ```bash
   # установка с вопросами (путь админ-панели, FlareSolverr)
   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash

   # сразу с обходом Cloudflare (FlareSolverr + cffetch в Docker)
   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --flaresolverr

   # без FlareSolverr и без вопросов
   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --no-flaresolverr --yes

   # на другом порту
   curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --port 9120
   ```

   Конкретная версия: `... | sudo bash -s -- --version v1.0.0`. Сборки статические (musl), от версии дистрибутива не зависят.

4. Ответьте на вопросы. Путь админ-панели:

   ```text
   Путь админ-панели:
     1) стандартный /admin
     2) своё название
   Выберите [1]:
   ```

   Своё название - один сегмент из строчных латинских букв, цифр, `-` и `_`, длиной 2-32 символа (например, `/my-panel`). Служебные пути (`/api`, `/cron`, `/stats`, `/docs` и другие) заняты. Если порт `9117` занят, установщик спросит порт, а без `--flaresolverr` / `--no-flaresolverr` - нужен ли обход Cloudflare.

5. Сохраните данные для входа, которые установщик выведет в конце:

   ```text
   ════════════════════════════════════════════════════════════
    Админ-панель: http://192.168.1.10:9117/my-panel?Z0mt0N7r2hoUM2TuOk
    Пароль (devkey): 9fQ2…
    Порт: 9117 (listenport в /opt/crabindex/init.yaml)
   ════════════════════════════════════════════════════════════
    Сохраните эти данные. Повторно: cd /opt/crabindex && sudo -u crabindex ./crabindex admin
   ```

6. Откройте адрес в браузере и введите пароль - см. [Админ-панель](admin.md). Проверьте службу:

   ```bash
   systemctl status crabindex
   curl -f http://127.0.0.1:9117/health          # {"status":"OK"}
   journalctl -u crabindex -f
   ```

Конфигурация - `/opt/crabindex/init.yaml`, база и логи - `/opt/crabindex/Data/`. Детали unit-файла, журналов и резервного копирования - на странице [Linux](deployment/linux.md).

:::note[FlareSolverr]
Обход Cloudflare (`--flaresolverr`) требует заметно больше ресурсов, чем сам CrabIndex: минимум 2 ядра и 4 ГБ памяти, рекомендуется 4 ядра и 8 ГБ. Лимиты контейнера установщик подбирает по серверу. На слабом сервере ставьте CrabIndex без него. Подробно: [требования к серверу](configuration/flaresolverr.md#требования-к-серверу).
:::

### Из клона репозитория

```bash
git clone https://github.com/sheinices/crabindex.git
cd crabindex
sudo scripts/install.sh
```

Готового комплекта в репозитории нет, поэтому установщик соберёт его сам: поставит `gcc`, `pkg-config`, `git`, Rust (через rustup) и Node.js 22, соберёт сервер, сайт, админ-панель и документацию и установит. Сборка занимает 10-15 минут. Если нужен git, а его ещё нет: `apt install git` (Debian/Ubuntu) или `dnf install git`.

### Из заранее собранного комплекта

```bash
make dist
sudo dist/install.sh
```

`make dist` собирает сайт, админ-панель, документацию и release-бинарник и складывает всё в `dist/`: `crabindex`, `wwwroot/`, шаблоны `Data/example.yaml`, `Data/example.conf`, `Data/crontab`, `Data/run-job.sh` и сам установщик `dist/install.sh`. Для сборки нужны Rust и Node.js, см. [Сборка из исходников](#from-source). Если на сервере мало памяти, соберите комплект на другой машине той же архитектуры и скопируйте каталог `dist/` (или его архив). Архивы релизов для Linux тоже содержат `install.sh`: распакуйте архив и запустите `sudo ./install.sh` из его каталога.

### Занятый порт {#busy-port}

По умолчанию CrabIndex слушает порт `9117`. Установщик проверяет его заранее (через `ss`, а если `ss` ещё нет - через `netstat` или пробное подключение):

- **новая установка, порт `9117` занят** - установщик сообщает об этом и предлагает ближайший свободный порт из диапазона `9118-9199`: Enter принимает предложенный, или можно ввести свой (1024-65535, свободный). С `--yes` свободный порт выбирается сам. Выбранный порт записывается в `listenport` в `init.yaml`;
- **`--port N`** - CrabIndex будет слушать этот порт. Если он занят другой программой, установка прерывается с ошибкой (при обновлении порт, занятый самим `crabindex`, не считается занятым);
- **обновление (`--update`)** или уже существующий `init.yaml` - порт из конфига сохраняется и не меняется.

Адреса в crontab (`http://127.0.0.1:9117/...`) установщик переписывает под выбранный порт. Итоговый порт виден в финальном сообщении (строка «Порт») и в `listenport` в `init.yaml`.

### Параметры установщика {#installer-options}

| Параметр | Что делает |
| --- | --- |
| `--version vX.Y.Z` | Скачать эту версию с GitHub Releases (без флага, если рядом нет ни комплекта, ни исходников, скачивается последняя) |
| `--from-source [DIR]` | Собрать из исходников (по умолчанию - репозиторий, в котором лежит скрипт) и установить. Недостающие инструменты сборки ставятся сами |
| `--check` | Только проверить систему: systemd, пакеты, архитектуру, место на диске, память, порт (`--port`, иначе `listenport` из существующего `init.yaml`, иначе `9117`). Ничего не меняет |
| `--no-deps` | Не устанавливать системные пакеты (только сообщить, чего не хватает) |
| `--bundle ПУТЬ\|URL` | Откуда брать файлы: каталог `make dist` или архив `.tar.gz` / `.tar.xz` (путь или URL, для URL нужен `curl`). По умолчанию - каталог самого скрипта, если в нём есть `crabindex` (так работает `dist/install.sh`), иначе `../dist` относительно скрипта |
| `--admin-path /x` | Путь админ-панели без вопроса |
| `--port N`, `--port=N` | Порт сервера (`listenport`, 1024-65535). Если порт занят другой программой, установка прерывается. Без флага на новой установке при занятом `9117` предлагается свободный порт, см. [Занятый порт](#busy-port) |
| `--yes`, `-y` | Не задавать вопросов: путь `/admin`, если не указан `--admin-path`; при занятом `9117` - первый свободный порт из `9118-9199` |
| `--flaresolverr` | Поставить FlareSolverr и cffetch в Docker для обхода Cloudflare, с лимитами CPU и памяти по ресурсам сервера. Ресурсоёмко, см. [FlareSolverr и cffetch](configuration/flaresolverr.md) |
| `--no-flaresolverr` | Не ставить обход Cloudflare и не спрашивать |
| `--update` | Обновить существующую установку: конфиг (включая порт) и данные сохраняются |
| `--uninstall` | Удалить службу, crontab, бинарник и `wwwroot/`. Данные и конфиг остаются |
| `--purge` | Вместе с `--uninstall`: удалить весь каталог установки (базу, конфиг, логи) и пользователя. Спрашивает подтверждение (или `--yes`) |
| `-h`, `--help` | Справка |

Переменные окружения `INSTALL_DIR` (по умолчанию `/opt/crabindex`) и `SERVICE_USER` (по умолчанию `crabindex`) меняют каталог и пользователя. При другом каталоге пути в crontab переписываются автоматически. Лимиты FlareSolverr задаются переменными `FLARESOLVERR_CPUS` и `FLARESOLVERR_MEMORY` (см. [Установка на сервер](configuration/flaresolverr.md#установка-на-сервер)).

```bash
sudo scripts/install.sh --bundle crabindex.tar.gz          # из архива
sudo dist/install.sh --admin-path /panel --yes              # без вопросов
sudo dist/install.sh --port 9120 --yes                      # на порту 9120
sudo INSTALL_DIR=/srv/crabindex dist/install.sh             # другой каталог
```

:::note[Примечание]
Если в каталоге установки уже есть `init.yaml`, установщик сохраняет его: заданные `admin.token`, `devkey` и `listenport` не трогает, дописывает только недостающие. Если вместо `init.yaml` используется `init.conf` (JSON), токен и `devkey` сгенерирует сам сервер при первом запуске, а установщик покажет их командой `crabindex admin`.
:::

Если CrabIndex уже установлен в каталоге, обычный запуск установщика сам переходит в режим обновления.

### Зависимости {#dependencies}

Перед установкой скрипт показывает сводку и ставит недостающее через `apt`, `dnf`, `yum`, `zypper`, `pacman` или `apk`:

| Что | Зачем |
| --- | --- |
| `curl`, `ca-certificates` | cron-задания (`Data/run-job.sh`), загрузка комплекта по URL |
| `tar`, `xz`, `gzip` | распаковка архивов комплекта |
| `rsync` | копирование комплектов и резервные копии |
| `cron` | расписание парсеров; служба включается автоматически |
| `flock` (`util-linux`) | защита cron-заданий от наложения |
| `ip`, `ss` (`iproute2`) | адрес сервера для ссылки на админ-панель, проверка занятости порта |

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
  ✗ порт 9117 занят - при новой установке установщик предложит другой порт (или --port N)
```

## Linux: вручную (systemd) {#linux-manual}

Если установщик не подходит, те же шаги можно выполнить руками.

1. Скачайте и распакуйте сборку под архитектуру сервера (`x86_64` или `arm64`; вместо архива можно взять каталог `dist/` после `make dist`):

   ```bash
   curl -fLO https://github.com/sheinices/crabindex/releases/latest/download/crabindex-linux-x86_64.tar.gz
   tar -xzf crabindex-linux-x86_64.tar.gz
   ```

2. Создайте пользователя и разложите файлы в `/opt/crabindex`:

   ```bash
   sudo useradd --system --home /opt/crabindex --shell /usr/sbin/nologin crabindex
   sudo mkdir -p /opt/crabindex
   sudo cp -a crabindex-*-linux-x86_64/. /opt/crabindex/
   sudo cp /opt/crabindex/Data/example.yaml /opt/crabindex/init.yaml
   sudo chown -R crabindex:crabindex /opt/crabindex
   sudo chmod 600 /opt/crabindex/init.yaml
   ```

3. Если порт `9117` занят, задайте другой в `/opt/crabindex/init.yaml` (`listenport: 9120`) и поправьте адреса в `Data/crontab`.

4. Создайте unit-файл `/etc/systemd/system/crabindex.service` (пример - на странице [Linux](deployment/linux.md#unit-systemd)) и запустите службу:

   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable --now crabindex
   ```

5. Установите расписание парсеров и получите данные для входа:

   ```bash
   sudo crontab -u crabindex /opt/crabindex/Data/crontab
   cd /opt/crabindex && sudo -u crabindex ./crabindex admin   # адрес админ-панели и devkey
   ```

:::note[Примечание]
`WorkingDirectory` в unit-файле обязателен: без него процесс не найдёт `init.yaml`, `Data/` и `wwwroot/`.
:::

Расписание парсеров не обязательно, если база наполняется синхронизацией (`syncapi`). Подробнее - [Cron](deployment/cron.md) и [Синхронизация](concepts/sync.md).

## Docker {#docker}

Готовый образ `ghcr.io/sheinices/crabindex` (amd64 и arm64) публикуется с каждым релизом, тег `latest` - последняя версия.

1. Установите Docker (на macOS и Windows - Docker Desktop).

2. Запустите контейнер одной командой:

   ```bash
   docker run -d --name crabindex --restart unless-stopped \
     -p 9117:9117 \
     -v crabindex-config:/app/config \
     -v crabindex-data:/app/Data \
     ghcr.io/sheinices/crabindex:latest
   ```

   Или через Docker Compose - вместе с FlareSolverr, WARP и cffetch для трекеров за Cloudflare:

   ```bash
   git clone https://github.com/sheinices/crabindex.git
   cd crabindex
   cp docker-compose.example.yml docker-compose.yml   # поправьте TZ, порты и лимиты
   docker compose up -d
   ```

   Если порт `9117` на хосте занят, опубликуйте другой: `-p 9120:9117` (в Compose - `"9120:9117"`).

3. Получите адрес админ-панели и пароль:

   ```bash
   docker logs crabindex 2>&1 | grep 'admin:'
   # admin: http://<host>:9117/admin?Z0mt0N7r2hoUM2TuOk
   # admin: devkey: 9fQ2…
   ```

   Замените `<host>` на адрес машины. Повторно: `docker exec crabindex ./crabindex admin`. Свой путь панели при первом запуске задаётся переменной `CRABINDEX_ADMIN_PATH` (`-e CRABINDEX_ADMIN_PATH=/my-panel`).

4. Проверьте: `curl -f http://localhost:9117/health` → `{"status":"OK"}`.

Конфигурация хранится в томе `/app/config` (`/app/config/init.yaml`), база - в томе `/app/Data`. Сборка образа из исходников, выбор конфига, cron для контейнера и полный стек - на странице [Docker](deployment/docker.md); пошаговое знакомство - в [Быстром старте](quickstart.md).

## macOS {#macos}

1. Скачайте архив со страницы [последнего релиза](https://github.com/sheinices/crabindex/releases/latest): `crabindex-macos-arm64.tar.gz` для Apple Silicon (M1 и новее) или `crabindex-macos-x86_64.tar.gz` для Intel. Архитектуру показывает `uname -m` (`arm64` или `x86_64`). Из терминала:

   ```bash
   curl -fLO https://github.com/sheinices/crabindex/releases/latest/download/crabindex-macos-arm64.tar.gz
   ```

2. Распакуйте архив и перенесите каталог, например, в `~/crabindex`:

   ```bash
   tar -xzf crabindex-macos-arm64.tar.gz
   mv crabindex-*-macos-arm64 ~/crabindex
   cd ~/crabindex
   ```

3. Если архив скачан через браузер, macOS (Gatekeeper) заблокирует запуск неподписанного бинарника. Снимите карантин:

   ```bash
   xattr -d com.apple.quarantine ./crabindex
   ```

4. Создайте конфиг из шаблона (при занятом порте `9117` поменяйте в нём `listenport`):

   ```bash
   cp Data/example.yaml init.yaml
   ```

5. Запустите сервер из этого каталога:

   ```bash
   ./crabindex
   ```

   Адрес админ-панели и пароль выводятся в консоль при первом запуске (`admin: http://…/admin?…`, `admin: devkey: …`), повторно - `./crabindex admin`.

6. Откройте `http://127.0.0.1:9117/`. Остановка - Ctrl+C (база сохраняется на диск).

Автозапуск через launchd и cron-задания - на странице [Windows и macOS](deployment/windows.md#macos).

## Windows {#windows}

1. Скачайте `crabindex-windows-x86_64.zip` со страницы [последнего релиза](https://github.com/sheinices/crabindex/releases/latest).

2. Распакуйте архив, например, в `C:\crabindex` (так, чтобы `crabindex.exe`, `wwwroot\` и `Data\` лежали прямо в этом каталоге).

3. Откройте PowerShell в этом каталоге и создайте конфиг из шаблона (при занятом порте `9117` поменяйте в нём `listenport`):

   ```powershell
   cd C:\crabindex
   Copy-Item Data\example.yaml init.yaml
   ```

4. Запустите сервер:

   ```powershell
   .\crabindex.exe
   ```

   Если SmartScreen предупредит о неизвестном издателе, выберите «Подробнее» → «Выполнить в любом случае». Брандмауэр может спросить разрешение на входящие подключения - разрешите, если к серверу будут обращаться с других устройств.

5. Адрес админ-панели и пароль выводятся в консоль при первом запуске, повторно - `.\crabindex.exe admin`. Откройте `http://127.0.0.1:9117/`.

Останавливайте сервер через Ctrl+C в консоли: так он сохраняет базу. Запуск как службы (NSSM), Планировщик заданий вместо cron и сборка из исходников - на странице [Windows и macOS](deployment/windows.md#windows).

## Сборка из исходников {#from-source}

### Требования

- **Rust** stable через [rustup](https://rustup.rs);
- **Node.js 22+** и npm - для сайта (`web/`), админ-панели (`admin/`) и документации (`docs/`);
- `git`, `make`, `bash`.

### Шаги сборки

1. Получите исходники:

   ```bash
   git clone https://github.com/sheinices/crabindex.git
   cd crabindex
   ```

2. Соберите веб-интерфейс и сервер:

   ```bash
   make web        # сайт → wwwroot/, админ-панель → wwwroot/admin/, документация → wwwroot/docs/
   make release    # target/release/crabindex
   ```

   Для кросс-сборки передайте целевую платформу: `make release TARGET=aarch64-unknown-linux-gnu` (toolchain и линкер для неё нужно установить отдельно). Готовый комплект для установки собирает `make dist`.

3. Создайте конфиг и запустите сервер из корня репозитория:

   ```bash
   cp Data/example.yaml init.yaml   # отредактируйте под себя
   ./target/release/crabindex
   ```

4. Проверьте:

   - сайт - `http://127.0.0.1:9117/`;
   - админ-панель - адрес и `devkey` выводятся в лог при первом запуске (`admin: http://…/admin?…`, `admin: devkey: …`), повторно - `./target/release/crabindex admin`;
   - проверка - `curl http://127.0.0.1:9117/health` → `{"status":"OK"}`;
   - версия сборки - `http://127.0.0.1:9117/version`.

Подробнее о целях `make` и структуре проекта - в разделе [Сборка](development/building.md). Сборка на Windows без `make` - на странице [Windows и macOS](deployment/windows.md#windows).

## Подготовка базы {#prepare-db}

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

**Linux (установщик).** Из готового релиза:

```bash
curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --update
```

Или из клона репозитория:

```bash
cd crabindex && git pull
make dist
sudo dist/install.sh --update
```

Установщик останавливает службу, заменяет бинарник, `wwwroot/` и шаблоны в `Data/` (`example.*`, `crontab`, `run-job.sh`), переустанавливает crontab и запускает службу снова. `init.yaml` (включая порт), база и логи не меняются. При остановке по SIGTERM сервер сам завершает фоновые задачи и сбрасывает на диск кеш FileDB и `masterDb`.

**Docker.** `docker pull ghcr.io/sheinices/crabindex:latest` и пересоздайте контейнер с теми же томами (в Compose - `docker compose pull && docker compose up -d`), см. [Docker](deployment/docker.md#обновление).

**macOS, Windows, ручная установка.** Остановите сервер, распакуйте новый архив и замените `crabindex` (`crabindex.exe`) и `wwwroot/`. `init.yaml` и `Data/` оставьте как есть.

:::warning[Внимание]
Перед обновлением сделайте резервную копию `/opt/crabindex/Data`. Если вы меняли `Data/crontab` или crontab пользователя `crabindex`, сохраните изменения: при обновлении они заменяются версией из комплекта.
:::

## Удаление

```bash
sudo dist/install.sh --uninstall           # служба, crontab, бинарник, wwwroot; данные остаются
sudo dist/install.sh --uninstall --purge   # всё, включая FileDB, конфиг и пользователя
```

Без клона репозитория: `curl -fsSL https://raw.githubusercontent.com/sheinices/crabindex/main/scripts/install.sh | sudo bash -s -- --uninstall`. Для `--purge` установщик спросит подтверждение в терминале (или добавьте `--yes`).

В Docker: `docker rm -f crabindex` и при необходимости `docker volume rm crabindex-config crabindex-data`. На macOS и Windows достаточно удалить каталог с программой.
