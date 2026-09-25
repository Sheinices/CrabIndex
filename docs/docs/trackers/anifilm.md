# AniFilm

![](/img/trackers/anifilm.ico)

AniFilm (slug `anifilm`) - аниме-трекер. CrabIndex обходит восемь категорий: `serials`, `ova`, `ona`, `movies`, `dorams`, `special`, `hentai` и `short-serials`. Раздачи из категории `dorams` получают тип `serial`, все остальные - `anime`. Для каждого релиза CrabIndex открывает страницу релиза, скачивает `.torrent` и строит из него магнет-ссылку.

Для страниц релизов и скачивания `.torrent` нужна сессия. Задайте cookie или логин и пароль.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Anifilm` |
| `host` | `https://anifilm.pro` |
| `alias` | не задан. Если задан, запросы и ссылки в FileDB строятся от `alias` |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс между страницами листинга |
| Авторизация | `cookie` или `login.u` / `login.p` |
| Кодировка | UTF-8 |
| Магнет-ссылки | из `.torrent` со страницы релиза |
| Учитывает `disable_trackers` | нет |

## Конфигурация

Вариант с cookie:

```yaml
Anifilm:
  host: https://anifilm.pro
  useproxy: false
  reqMinute: 8
  log: true
  cookie: "XSRF-TOKEN=TOKEN; anifilm_session=SESSION"
```

Вариант с логином и паролем:

```yaml
Anifilm:
  host: https://anifilm.pro
  useproxy: false
  reqMinute: 8
  log: true
  login:
    u: "mylogin"
    p: "mypassword"
```

### Как CrabIndex выбирает авторизацию

1. Сначала используется сессия, полученная входом по логину, если она есть.
2. Иначе используется `cookie` из конфига. Если он задан, CrabIndex не входит по логину.
3. Если `cookie` пуст, а `login.u` задан, CrabIndex загружает `{host}/account/login`, берёт CSRF-токен из формы и отправляет `LoginForm[username]` / `LoginForm[password]`. Попытки входа не чаще одной в 2 минуты.

Если вместо страницы приходит форма входа, сессия, полученная по логину, сбрасывается, и при следующем запуске CrabIndex входит заново.

### Как получить cookie

1. Войдите на anifilm.pro в браузере.
2. Откройте DevTools (F12) → Application → Cookies.
3. Скопируйте `XSRF-TOKEN` и `anifilm_session` в формате `XSRF-TOKEN=…; anifilm_session=…`.

## Cron-маршруты

Путь и имена параметров не зависят от регистра. Маршрут принимает только `GET`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/anifilm/parse` | `fullparse` (`true`/`false`, по умолчанию `false`) | `ok` после завершения, `work` если parse уже идёт, `config missing` | Синхронно обходит листинги `{host}/releases/page/N?category=…` |

Сколько страниц обходится в каждой категории:

| Категория | Обычный режим | `fullparse=true` |
| --- | --- | --- |
| `serials` | 2 | 70 |
| `ova` | 2 | 32 |
| `ona` | 2 | 2 |
| `movies` | 2 | 17 |
| `dorams` | 2 | 10 |
| `special`, `hentai`, `short-serials` | 2 | 5 |

В режиме `fullparse` дата раздачи из страницы `N` листинга считается равной «сейчас минус 2·N дней», чтобы старые релизы не выглядели свежими.

## Рекомендуемое расписание

```crontab
# раз в сутки, таймаут 30 минут
55 6 * * *  /opt/crabindex/Data/run-job.sh anifilm-parse http://127.0.0.1:9117/cron/anifilm/parse 1800
```

Для первичного заполнения вручную вызовите `…/cron/anifilm/parse?fullparse=true`. Проход долгий, поэтому используйте большой таймаут curl.

## Файлы в Data/temp

Нет. Сессия после входа по логину хранится только в памяти.

## Особенности и ограничения

- Страница релиза и `.torrent` запрашиваются только для новых раздач, при пустом магнете в базе или при изменившемся заголовке.
- Если на странице релиза есть 1080p-версия, к заголовку добавляется ` [1080p]`.
- Если заданный в конфиге `cookie` устарел, CrabIndex не переключается на логин: пока в конфиге есть `cookie`, по логину он не входит. Обновите cookie или удалите его.
- Вход по логину всегда идёт напрямую, без прокси.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/anifilm/parse"
tail -f /opt/crabindex/Data/log/anifilm.log   # "Login OK", "Login failed - CSRF token not found", "tid not found"
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Решение проблем](../operations/troubleshooting.md)
