# Korsars

![](/img/trackers/korsars.ico)

Korsars (slug `korsars`) - трекер на движке phpBB с фильмами, сериалами и мультфильмами. CrabIndex обходит 24 раздела: 6 с фильмами (`movie`), 12 с сериалами (`serial`) и 6 с мультфильмами (`multfilm` и `multserial`). Магнет-ссылки есть прямо в листинге раздела, поэтому страницы тем и `.torrent` не запрашиваются.

Листинги доступны только авторизованным пользователям. Нужен cookie `bb_data`: задайте его вручную или укажите логин и пароль, чтобы CrabIndex получил cookie сам.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Korsars` |
| `host` | `https://korsars.pro` |
| `alias` | не задан. Запросы идут на `alias`, а ссылки в FileDB остаются на `host` |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс |
| Авторизация | `cookie` (`bb_data=…`) или `login.u` / `login.p` |
| Кодировка | UTF-8 |
| Магнет-ссылки | из листинга раздела |
| Учитывает `disable_trackers` | да, все четыре маршрута возвращают `disabled` |

## Конфигурация

Вариант со статическим cookie:

```yaml
Korsars:
  host: https://korsars.pro
  alias: ""        # необязательное зеркало
  useproxy: false
  reqMinute: 8
  log: true
  cookie: "bb_data=1-USER_ID-HASH-IP-UA_HASH"
```

Вариант с логином и паролем:

```yaml
Korsars:
  host: https://korsars.pro
  useproxy: false
  reqMinute: 8
  log: true
  login:
    u: "mylogin"
    p: "mypassword"
```

### Как CrabIndex выбирает авторизацию

1. Если после входа по логину есть сохранённая сессия и ей меньше 24 часов, используется она.
2. Иначе используется `cookie` из конфига. Если он задан, CrabIndex не входит по логину.
3. Если `cookie` пуст, CrabIndex отправляет `POST {alias или host}/login.php` с `login.u` / `login.p`. Вход считается успешным, только если в ответе есть cookie `bb_data`.

Если вместо листинга приходит форма входа, сессия, полученная по логину, сбрасывается, и при следующем запросе CrabIndex входит заново. На `cookie` из конфига это не влияет.

### Как получить cookie

1. Войдите на korsars.pro в браузере.
2. Откройте DevTools (F12) → Application → Cookies.
3. Скопируйте значение `bb_data`. Оно состоит из пяти частей через дефис: `1-USER_ID-HASH-IP-UA_HASH`.
4. Укажите его в формате `cookie: "bb_data=ЗНАЧЕНИЕ"`.

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Все маршруты принимают `GET` и `POST`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/korsars/parse` | `page` (по умолчанию `0`) - номер страницы с нуля | текстовый лог `раздел - страница`, `work`, `login failed`, `config missing` | Синхронно обходит одну страницу каждого из 24 разделов |
| `/cron/korsars/UpdateTasksParse` | нет | `ok` / `work` / `disabled` сразу | В фоне обновляет карту страниц по пейджерам разделов |
| `/cron/korsars/ParseAllTask` | нет | `ok` / `work` / `disabled` сразу | В фоне обходит все страницы карты в рамках цикла |
| `/cron/korsars/ParseLatest` | `pages` (по умолчанию `5`, ≤ 0 → 5) | лог обойдённых страниц или `ok`, `work`, `login failed` | Первые `pages` страниц каждого раздела |

Поведение фоновых задач:

- `ParseAllTask` начинает новый круг только когда pending = 0. После рестарта цикл продолжает `/cron/maintenance/ResumeParseAll`. Если 45 минут нет прогресса, задачу останавливает сторож.
- `UpdateTasksParse` ограничен 30 минутами.
- `ParseLatest`, `ParseAllTask` и `UpdateTasksParse` взаимоисключающие. Часовой `parse` имеет приоритет.
- Если вход не удался, фоновая задача пишет в лог `login failed` и завершается.
- Если карта пуста, `ParseAllTask` и `ParseLatest` сначала строят её сами.

## Рекомендуемое расписание

```crontab
# минуты 15/27/45/57, чтобы не пересекаться с kinozal (:03, :18, :33, :48)
15,27,45,57 * * * *  /opt/crabindex/Data/run-job.sh korsars-parse http://127.0.0.1:9117/cron/korsars/parse 900

45 2 * * *  /opt/crabindex/Data/run-job.sh korsars-UpdateTasksParse http://127.0.0.1:9117/cron/korsars/UpdateTasksParse 60

# полный обход (~3,6 ч); новый круг только при pending = 0
53 5,17 * * *  /opt/crabindex/Data/run-job.sh korsars-ParseAllTask http://127.0.0.1:9117/cron/korsars/ParseAllTask 60

15 7 * * *  /opt/crabindex/Data/run-job.sh korsars-ParseLatest http://127.0.0.1:9117/cron/korsars/ParseLatest 900
```

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/korsars_taskParse.json` | Карта страниц: раздел → страницы |
| `Data/temp/korsars_parseAllCycle.json` | Состояние цикла `ParseAllTask` |

Сессия, полученная по логину, хранится только в памяти и после рестарта запрашивается заново.

## Особенности и ограничения

- На странице листинга 50 тем. Последняя страница берётся из максимального `start=N` в пейджере. Если пейджер сжался, лишние страницы удаляются из карты.
- Раздачи без магнета и с некорректным infohash отбрасываются.
- Если заданный в конфиге `cookie` устарел, CrabIndex не переключается на логин: пока в конфиге есть `cookie`, по логину он не входит. Обновите cookie или удалите его, оставив `login`.
- Вход по логину всегда идёт напрямую, без прокси, даже при `useproxy: true`.
- `parse` выдерживает `parseDelay` перед каждым разделом, поэтому один проход занимает около 3 минут.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/korsars/parse"
curl "http://127.0.0.1:9117/cron/korsars/ParseLatest?pages=1"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
tail -f /opt/crabindex/Data/log/korsars.log   # ищите "Login OK" / "Login FAILED"
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
