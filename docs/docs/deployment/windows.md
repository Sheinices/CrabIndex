# Windows и macOS

Основная и проверенная платформа CrabIndex - Linux (systemd + cron) и Docker. Код сервера при этом кроссплатформенный: платформо-зависимая часть одна - обработка сигналов остановки. На Windows и macOS сервер собирается из исходников обычным `cargo build`; готового установщика и службы «из коробки» нет.

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Сборка `cargo build` | да | да | да |
| Остановка по Ctrl+C с сохранением FileDB | да | да | да |
| Остановка по SIGTERM с сохранением FileDB | да | да | - (сигнала нет) |
| `Data/run-job.sh` и `Data/crontab` | да | да (cron есть, `flock` нужно поставить) | нет - используйте Планировщик заданий |
| `make`, `scripts/build-web-ui.sh` | да | да | через Git Bash / WSL или вручную |
| Регулярно тестируется | да | нет | нет |

:::tip[Совет]
На Windows и macOS самый простой путь - Docker Desktop: см. [Docker](docker.md). Нативный запуск ниже подходит для разработки и небольших домашних установок.
:::

## macOS

### Сборка

```bash
# Rust: https://rustup.rs ; Node.js 22+: brew install node
git clone https://github.com/sheinices/crabindex.git
cd crabindex
make dist            # dist/: crabindex, wwwroot/, Data/
```

### Запуск

```bash
mkdir -p ~/crabindex && cp -a dist/. ~/crabindex/
cd ~/crabindex
cp Data/example.yaml init.yaml
./crabindex
```

Процесс работает относительно текущего каталога - запускайте его из каталога, где лежат `init.yaml`, `Data/` и `wwwroot/`.

### Автозапуск через launchd

```xml
<!-- ~/Library/LaunchAgents/com.crabindex.plist -->
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.crabindex</string>
  <key>ProgramArguments</key>
  <array><string>/Users/YOU/crabindex/crabindex</string></array>
  <key>WorkingDirectory</key><string>/Users/YOU/crabindex</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/Users/YOU/crabindex/Data/log/console.log</string>
  <key>StandardErrorPath</key><string>/Users/YOU/crabindex/Data/log/console.log</string>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/com.crabindex.plist
```

launchd останавливает процесс сигналом SIGTERM - FileDB будет сохранена.

Для cron-заданий из `Data/crontab` поправьте пути (`/opt/crabindex/…` → ваш каталог) и установите `flock` (например, `brew install flock`), который нужен `run-job.sh`.

## Windows

### Требования

- Rust через [rustup-init.exe](https://rustup.rs) (toolchain `x86_64-pc-windows-msvc` и Build Tools for Visual Studio);
- Node.js 22+ - если нужен веб-интерфейс;
- Git.

### Сборка

`make` и bash-скрипты на Windows без Git Bash или WSL не работают, поэтому соберите вручную в PowerShell:

```powershell
git clone https://github.com/sheinices/crabindex.git
cd crabindex

# Сервер
cargo build --release --locked -p crabindex

# Веб-интерфейс → wwwroot\
cd web
npm ci
npm run build
cd ..
Remove-Item -Recurse -Force wwwroot -ErrorAction SilentlyContinue
Copy-Item -Recurse web\dist wwwroot
```

### Раскладка и запуск

```powershell
New-Item -ItemType Directory C:\crabindex, C:\crabindex\Data | Out-Null
Copy-Item target\release\crabindex.exe C:\crabindex\
Copy-Item -Recurse wwwroot C:\crabindex\wwwroot
Copy-Item Data\example.yaml C:\crabindex\init.yaml
Copy-Item Data\crontab C:\crabindex\Data\    # как справка по расписанию

cd C:\crabindex
.\crabindex.exe
```

Проверьте `http://127.0.0.1:9117/health`. Рабочий файл конфигурации - `init.yaml` в **текущем каталоге** процесса.

### Остановка и сохранение данных

На Windows сервер реагирует только на Ctrl+C в консоли: получив его, он завершает фоновые задачи и сохраняет FileDB. Если процесс завершают принудительно (`Stop-Process`, закрытие окна, `TerminateProcess` у менеджеров служб), сохранение не выполняется - несохранённые изменения индекса `masterDb` с момента последнего автосохранения (раз в 10 минут) будут потеряны.

Перед плановой остановкой сбросьте индекс вручную:

```powershell
Invoke-WebRequest -UseBasicParsing http://127.0.0.1:9117/jsondb/save
```

### Работа как служба

Встроенной поддержки служб Windows нет. Можно использовать сторонний менеджер, например [NSSM](https://nssm.cc) - он сначала посылает приложению Ctrl+C:

```cmd
nssm install CrabIndex "C:\crabindex\crabindex.exe"
nssm set CrabIndex AppDirectory "C:\crabindex"
nssm set CrabIndex AppStopMethodConsole 60000
nssm start CrabIndex
```

### Планировщик вместо cron

`run-job.sh` на Windows не работает. Создайте задания Планировщика, которые вызывают нужные адреса из `Data/crontab`:

```powershell
$action = New-ScheduledTaskAction -Execute "powershell.exe" `
  -Argument '-NoProfile -Command "Invoke-WebRequest -UseBasicParsing http://127.0.0.1:9117/cron/rutor/parse -TimeoutSec 900"'
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 15)
Register-ScheduledTask -TaskName "crabindex-rutor-parse" -Action $action -Trigger $trigger
```

Запросы с `127.0.0.1` не требуют `devkey`. Расписание и назначение задач - в разделе [Cron](cron.md).

:::note[Примечание]
Сервисы FlareSolverr и cffetch выпускаются как Docker-образы. На Windows и macOS их удобнее запускать в Docker Desktop, а в `init.yaml` указать `http://127.0.0.1:8191/v1` и `http://127.0.0.1:8192/fetch` с опубликованными портами.
:::
