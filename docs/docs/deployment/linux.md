# Linux

Linux - основная платформа для CrabIndex в продакшене: systemd управляет процессом, cron запускает парсеры. Бинарник статически не привязан к системным библиотекам TLS (используется rustls), поэтому дополнительные пакеты не нужны.

Проще всего установить CrabIndex установщиком `scripts/install.sh` (`sudo dist/install.sh` после `make dist`): он создаёт пользователя, unit systemd, crontab и данные для входа в админ-панель. Процедура описана в разделе [Установка](../installation.md). Здесь - детали того, что получается в итоге.

## Раскладка файлов

| Путь | Назначение |
| --- | --- |
| `/opt/crabindex/crabindex` | Бинарник |
| `/opt/crabindex/init.yaml` | Runtime-конфигурация (рабочий каталог процесса) |
| `/opt/crabindex/wwwroot/` | Сайт, админ-панель `admin/`, документация `docs/`, `openapi.yaml` |
| `/opt/crabindex/Data/` | FileDB, логи, состояние задач, `crontab`, `run-job.sh` |
| `/opt/crabindex/Data/temp/admin.secret` | Секрет для cookie входа в админ-панель (создаётся автоматически) |
| `/etc/systemd/system/crabindex.service` | Unit systemd |

## Unit systemd

Установщик создаёт такой unit (при ручной установке можно взять его за основу):

```ini
# /etc/systemd/system/crabindex.service
[Unit]
Description=CrabIndex torrent aggregator
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=crabindex
Group=crabindex
WorkingDirectory=/opt/crabindex
ExecStart=/opt/crabindex/crabindex
Restart=on-failure
RestartSec=5
# Время на сброс FileDB при остановке
TimeoutStopSec=60
# Много открытых шардов при большой базе
LimitNOFILE=65536
UMask=0027
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
ReadWritePaths=/opt/crabindex

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now crabindex
```

Процессу нужна запись в весь рабочий каталог: сервер сохраняет туда `init.yaml` (из админ-панели и при генерации токена) и данные в `Data/`.

При `systemctl stop` процесс получает SIGTERM, до 30 секунд ждёт завершения текущих запросов и затем сохраняет изменённые шарды и `masterDb`. `TimeoutStopSec` оставляет на это запас.

## Управление

```bash
systemctl status crabindex
sudo systemctl restart crabindex
journalctl -u crabindex -n 100 --no-pager
journalctl -u crabindex -f
curl -f http://127.0.0.1:9117/health
```

После изменения `init.yaml` перезапуск не нужен (кроме `listenip` / `listenport`).

Адрес админ-панели и `devkey`:

```bash
cd /opt/crabindex && sudo -u crabindex ./crabindex admin
```

## Cron

Установщик ставит crontab сам. Вручную:

```bash
sudo crontab -u crabindex /opt/crabindex/Data/crontab
sudo crontab -u crabindex -l
```

Задания вызывают `/opt/crabindex/Data/run-job.sh`, которому нужны `bash`, `curl` и `flock` (пакет `util-linux`). Подробности - [Cron](cron.md).

## Сборка на сервере

```bash
curl https://sh.rustup.rs -sSf | sh     # Rust
# Node.js 22+ из пакетов дистрибутива или nodesource/nvm
git clone https://github.com/sheinices/crabindex.git && cd crabindex
make dist
```

Если на сервере мало памяти, соберите на другой машине той же архитектуры (или с `make release TARGET=…`) и скопируйте каталог `dist/` или его архив. Установщик принимает и архив: `sudo ./install.sh --bundle crabindex.tar.gz`.

## Обновление

```bash
cd ~/crabindex && git pull && make dist
sudo dist/install.sh --update
curl -s http://127.0.0.1:9117/version
```

Установщик останавливает службу, заменяет бинарник, `wwwroot/`, `Data/example.*`, `Data/crontab`, `Data/run-job.sh`, переустанавливает crontab пользователя и запускает службу. `init.yaml`, путь админ-панели, токен, `devkey` и база сохраняются.

Вручную:

```bash
sudo systemctl stop crabindex
sudo install -m 0755 -o crabindex -g crabindex dist/crabindex /opt/crabindex/crabindex
sudo rm -rf /opt/crabindex/wwwroot
sudo cp -a dist/wwwroot /opt/crabindex/wwwroot
sudo chown -R crabindex:crabindex /opt/crabindex/wwwroot
sudo systemctl start crabindex
```

:::warning[Внимание]
Сохраните резервную копию `/opt/crabindex/Data` перед обновлением. Никогда не заменяйте `Data/fdb` и `Data/masterDb.bz` файлами из бандла.
:::

## Резервное копирование

Достаточно копировать `Data/fdb/`, `Data/masterDb.bz` (и дневные копии `masterDb_*.bz`), `Data/temp/` (состояние задач и синхронизации), `Data/tracks/` и `init.yaml`. Для согласованной копии сначала сбросьте индекс или остановите сервис:

```bash
curl -s http://127.0.0.1:9117/jsondb/save
sudo tar -C /opt/crabindex -czf crabindex-backup.tgz Data/fdb Data/masterDb.bz Data/temp Data/tracks init.yaml
```

## Ограничение доступа

Если сервер доступен из интернета:

- задайте `apikey` ([Аутентификация](../authentication.md));
- слушайте только loopback (`listenip: 127.0.0.1`) и публикуйте CrabIndex через [обратный прокси](reverse-proxy.md) с HTTPS - особенно если вы входите в [админ-панель](../admin.md) через интернет;
- выберите свой путь админ-панели и не публикуйте адрес входа.

## Удаление

```bash
sudo dist/install.sh --uninstall           # данные и конфиг остаются в /opt/crabindex
sudo dist/install.sh --uninstall --purge   # удалить всё, включая FileDB и пользователя
```
