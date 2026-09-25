# AnimeLayer

![](/img/trackers/animelayer.ico)

AnimeLayer (slug `animelayer`, около 6 тысяч раздач) - аниме-трекер. Листинг `/torrents/anime/` доступен только авторизованным пользователям, поэтому CrabIndex работает с cookie или логином/паролем. Магнет и размер берутся из `.torrent`, который скачивается со страницы раздачи (`…/download/`).

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Animelayer` |
| `host` | `https://animelayer.ru` (схема всегда приводится к `https://`) |
| `alias` | не используется |
| `reqMinute` | `8` (задержка `parseDelay` = 7000 мс) |
| Авторизация | обязательна: `cookie` и/или `login.u` / `login.p` |
| Кодировка | UTF-8 |
| Магнет-ссылки | из скачанного `.torrent` |
| Учитывает `disable_trackers` | нет |

## Конфигурация

```yaml
Animelayer:
  host: https://animelayer.ru
  useproxy: false
  reqMinute: 8
  log: true
  cookie: ""          # статический cookie (layer_hash=…; layer_id=…)
  login:              # логин и пароль - рекомендуемый вариант
    u: "mylogin"
    p: "mypassword"
```

Порядок авторизации при каждом запуске `parse`:

1. Если задан `cookie`, CrabIndex проверяет его запросом к `/torrents/anime/`. Cookie считается рабочим, если на странице нет формы входа.
2. Если cookie нет или он не прошёл проверку, выполняется вход через `POST /auth/login/` с `login.u` / `login.p`. Полученные `layer_hash`, `layer_id` (и `PHPSESSID`, если есть) хранятся в памяти 24 часа.
3. Если страница листинга вернулась пустой, cookie сбрасывается и выполняется повторная авторизация (один повтор на страницу).

Рекомендуется задавать логин и пароль: тогда CrabIndex сам обновляет сессию. Со статическим cookie после его истечения потребуется ручная замена.

### Как получить cookie

1. Войдите на `https://animelayer.ru` в браузере.
2. DevTools (F12) → Application → Cookies → `animelayer.ru`.
3. Скопируйте `layer_hash` и `layer_id` и соберите строку `layer_hash=…; layer_id=…`.

## Cron-маршруты

Пути и имена параметров нечувствительны к регистру. Маршруты принимают GET и POST.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/animelayer/parse` | `parseFrom` (по умолчанию 0 → страница 1), `parseTo` (по умолчанию 0 → равно `parseFrom`) | `ok`, `work`, `work_login`, текст ошибки авторизации | Страницы `/torrents/anime/?page=N` в диапазоне. Нумерация с 1; если `parseFrom > parseTo`, границы меняются местами |
| `/cron/animelayer/TakeLogin` | - | `true` / `false` (JSON) | Принудительный вход по `login.u` / `login.p`. Удобно для проверки учётных данных |

Ответы `parse` при проблемах с авторизацией:

- `work_login` - вход не удался или полученный cookie не прошёл проверку;
- `Failed to authorize, please provide either cookie or credentials` - не задан ни рабочий cookie, ни логин.

## Рекомендуемое расписание

```crontab
# animelayer-parse -> /cron/animelayer/parse (max_time=900s)
2,17,32,47 * * * *  /opt/crabindex/Data/run-job.sh animelayer-parse http://127.0.0.1:9117/cron/animelayer/parse 900
```

Каждые 15 минут разбирается первая страница листинга. Для первичного наполнения базы запустите вручную с диапазоном страниц.

## Файлы в Data/temp

Нет. Сессия хранится только в памяти.

## Особенности и ограничения

- Если раздача уже есть в базе с тем же заголовком, `.torrent` повторно не скачивается.
- Если вместо `.torrent` приходит HTML, раздача помечается ошибочной (`cookie is likely not authorized`).
- Авторизация выполняется до захвата блокировки парсера, поэтому даже при ответе `work` может пройти проверочный запрос к сайту.

## Диагностика

```bash
# Проверить логин/пароль
curl "http://127.0.0.1:9117/cron/animelayer/TakeLogin"

# Ручной запуск
curl "http://127.0.0.1:9117/cron/animelayer/parse"

# Первичное наполнение
curl "http://127.0.0.1:9117/cron/animelayer/parse?parseFrom=1&parseTo=50"

tail -f Data/log/animelayer.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
