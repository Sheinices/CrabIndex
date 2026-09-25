# Решение проблем

Начните с трёх проверок: жив ли процесс, отвечает ли `/health` и что в логах.

```bash
systemctl status crabindex                 # или: docker ps
curl -f http://127.0.0.1:9117/health
journalctl -u crabindex -n 200 --no-pager  # или: docker logs --tail 200 crabindex
```

## Сервер не запускается

**Порт занят.** В логе: `[fatal] cannot listen on 0.0.0.0:9117`.

```bash
ss -ltnp | grep ':9117'
```

Освободите порт или поменяйте `listenport`.

**Неверный `listenip`.** В логе: `[fatal] invalid listenip '…'`. Допустимы `any`, пусто или IP-адрес (`127.0.0.1`, `::`, `192.168.1.10`).

**Конфиг не читается.** В логе категории `config` - ошибка разбора `init.yaml` / `init.conf`, сервер работает на значениях по умолчанию. Проверьте YAML на отступы и табуляции. В `init.conf` (JSON) допустимы комментарии `//` и `/* */`, но не висячие запятые.

**Не тот рабочий каталог.** Сервер ищет `init.yaml`, `Data/` и `wwwroot/` в текущем каталоге. Для systemd обязателен `WorkingDirectory=/opt/crabindex`.

**Docker: нет прав.** Контейнер работает от UID `1000`. Каталоги, смонтированные с хоста, должны быть доступны ему на запись: `sudo chown -R 1000:1000 config data`.

## Веб-интерфейс пустой или 404

- Не собран `wwwroot/`: выполните `make web` (или используйте бандл `make dist` / Docker). Ошибка в логе: `wwwroot/index.html: …`.
- `web: false` отключает раздачу файлов сайта.
- `make web` пересоздаёт `wwwroot/` целиком и сам пересобирает админ-панель и документацию. Если после ручной сборки только `web/` пропали `/docs/` или панель, выполните `make web` полностью.
- Страниц `/settings` и `/jobs` больше нет: настройки и фоновые задачи находятся в [админ-панели](../admin.md). Старые закладки ведут на `404` (или `401`, если задан `apikey`).
- Если после обновления браузер показывает старый интерфейс, обновите страницу ещё раз: `/sw.js` удалит service worker прежней версии и её кеши.

## Админ-панель не открывается

| Симптом | Причина и решение |
| --- | --- |
| `404` на `/admin` или своём пути | Адрес открыт без токена или с неверным токеном, путь или токен изменены, `admin.enable: false` или прокси отбрасывает строку запроса. Актуальный адрес покажет `crabindex admin` |
| `401` на `/admin` | То же, что `404`, если на сервере задан `apikey` |
| `503 admin UI is not built` | Нет `wwwroot/admin/index.html`: выполните `make web` или используйте `make dist` / Docker |
| «Неверный ключ» при входе | Введите текущий `devkey` из конфига |
| `429`, «Слишком много попыток» | 5 неудачных входов с одного IP за 10 минут. Подождите до 10 минут |
| После перезапуска снова просит ключ | Сессии хранятся в памяти и сбрасываются при перезапуске. Это нормально |
| Потерян адрес или ключ | `cd /opt/crabindex && sudo -u crabindex ./crabindex admin` или `docker exec crabindex ./crabindex admin` |

Подробнее - [Админ-панель](../admin.md#решение-проблем).

## Поиск ничего не находит

1. База пуста? `curl http://127.0.0.1:9117/lastupdatedb` - дата `01.01.2000 01:01` означает, что в `masterDb` нет ни одного бакета.
2. Если шарды в `Data/fdb/` есть, а поиск пуст после рестарта - проверьте `Data/masterDb.bz` (дата изменения, права) и лог `fdb`. Без индекса шарды невидимы.
3. Синхронизация идёт? В логе категории `sync` должны быть строки `start`, `[N] time=…`, `end`. Проверьте `syncapi` и доступность `https://sync.crab.rip/sync/conf`.
4. Поиск по `tt…` / `kp…` пуст - проверьте блок [alloha](../configuration/alloha.md).
5. Трекер в `disable_trackers` или раздачи отфильтрованы по сезону/категории - см. [Конфигурация поиска](../configuration/search.md).

## FileDB не обновляется

```bash
crontab -l -u crabindex
curl http://127.0.0.1:9117/health/background-jobs
tail -f /opt/crabindex/Data/log/rutor.log
```

- В Docker-образе нет cron: задания нужно ставить на хосте.
- `run-job.sh` пишет `SKIP … previous run still active` - предыдущий запуск ещё не закончился.
- Ответ `work` у `parse` - parse этого трекера уже идёт. На уровне Debug категория `trackers` пишет `{трекер}: parse skipped (work, lock held for Ns)` - сколько секунд держится блокировка. Если она держится часами, а в журнале трекера нет прогресса, перезапустите сервер.
- Ответ `disabled` - трекер в `disable_trackers`.
- `/jsondb/save` отвечает `syncapi` - на инстансе с синхронизацией это нормально.

## ParseAll остановился после рестарта

Состояние цикла лежит в `Data/temp/{трекер}_parseAllCycle.json`, карта страниц - в `Data/temp/{трекер}_taskParse.json`. Примерно через 45 секунд после старта сервер сам продолжает незавершённые циклы; задание `parseall-resume` (каждые 15 минут) - запасной путь.

```bash
curl http://127.0.0.1:9117/cron/maintenance/ParseAllStatus
curl http://127.0.0.1:9117/cron/maintenance/ResumeParseAll
curl http://127.0.0.1:9117/health/background-jobs
```

Если необработанных страниц не осталось, Resume ничего не делает - новый круг начнёт следующий запуск `ParseAllTask` по расписанию.

## ParseAll ходит по пустым страницам

Карта страниц выросла дальше реального конца листинга. Запустите `UpdateTasksParse` этого трекера: он перечитает пагинатор и удалит лишние страницы. Пустой ответ трекера карту не обнуляет.

```bash
curl http://127.0.0.1:9117/cron/rutor/UpdateTasksParse
grep pruned /opt/crabindex/Data/log/rutor.log
```

Страница, трижды подряд не загрузившаяся, пропускается циклом: `ParseAll skip slot page=… after 3 failures`.

## 401 или 403

| Код | Причина |
| --- | --- |
| `401` на поиске | `apikey` задан, а клиент не передал его или передал неверный |
| `401` на `/cron`, `/dev`, `/jsondb` | Запрос не из локальной сети (или через прокси), `devkey` не передан или неверен |
| `404` на `/api/v1.0/config` | Config API доступен только через админ-панель: `{admin.path}/api/config/*`, см. [Config API](../api-reference/config.md) |
| `403` на тех же путях | `devkey` на сервере не задан, а запрос не из локальной сети |

Через обратный прокси административные пути всегда требуют `devkey`. См. [Матрица доступа](access-matrix.md).

```bash
curl -H "X-Dev-Key: DEVKEY" http://127.0.0.1:9117/cron/maintenance/Status
curl "http://127.0.0.1:9117/api/v1.0/conf?apikey=KEY"   # "apikey": true - ключ принят
```

## Rutracker / Kinozal: 403 или «Just a moment…»

1. Проверьте FlareSolverr и cffetch:

   ```bash
   curl -s http://127.0.0.1:8191/
   curl -s http://127.0.0.1:8192/health
   curl -s http://127.0.0.1:9117/cron/cloudflare/Warmup
   ```

2. Если CrabIndex в Docker, а FlareSolverr в `network_mode: host`, адреса должны быть `http://host.docker.internal:…` - см. [FlareSolverr и cffetch](../configuration/flaresolverr.md).
3. На VPS с IP дата-центра Cloudflare может не выдавать cookie даже браузеру - выпускайте FlareSolverr через WARP.
4. `cffetch.proxy` должен совпадать с `PROXY_URL` FlareSolverr: cookie `cf_clearance` привязана к IP.

Первый прогрев может длиться несколько минут. В логе `host`: `… закрыт проверкой Cloudflare, переходим на браузер` - хост переключён на FlareSolverr.

## 403 или 503 без признаков Cloudflare

В логе `host` один раз на хост:

```text
{host}: 403 без признаков Cloudflare - браузерный fallback не сработает; проверьте Referer/зеркало/лимит
```

Это не проверка Cloudflare, и FlareSolverr не поможет. Причины: трекер требует определённый `Referer` (например, Ultradox), сменилось зеркало (поправьте `host` / `alias`), превышен лимит запросов (уменьшите `reqMinute`) или истекла cookie.

## Прокси или Tor не работает

```bash
ss -ltnp | grep ':9050'
```

- Для SOCKS5 указывайте схему: `socks5://127.0.0.1:9050`. Адрес без схемы считается HTTP-прокси.
- Проверьте регулярное выражение `globalproxy.pattern` - оно сравнивается с полным URL.
- `useproxy: true` работает только при непустом `proxy.list`.

См. [Прокси](../configuration/proxy.md).

## Настройки из админ-панели пропадают (Docker)

`/app/init.yaml` в контейнере - символьная ссылка на `/app/config/init.yaml`, и админ-панель сохраняет прямо в том. Если изменения всё же пропадают, проверьте, что том `/app/config` смонтирован (см. `docker-compose.example.yml`) и доступен на запись пользователю контейнера (uid 1000).

## Высокое потребление памяти

- `evercache.enable: true` с `validHour: 0` держит всю базу в памяти. Для большой базы используйте `validHour: 1` или выключите `evercache`.
- Уменьшите `evercache.maxOpenWriteTask` и `maxreadfile`.
- Каждая сессия FlareSolverr (Chromium) занимает сотни мегабайт; второй экземпляр (`crawlUrl`) - ещё столько же.
- Ограничьте память контейнера в compose (`deploy.resources.limits.memory`).

## Растёт диск

- Журнал FileDB `Data/log/fdb.*.log` при первой синхронизации растёт очень быстро - выключите `logFdb` или задайте `logFdbMaxSizeMb` / `logFdbMaxFiles`.
- Журналы парсеров `Data/log/{трекер}.log` и `tracks.log` не ротируются - настройте `logrotate`.

## Данные повреждены

Запустите проверку в режиме `report` и исправьте по отчёту - см. [Обслуживание FileDB](maintenance.md).
