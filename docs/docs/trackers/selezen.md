# Selezen

![](/img/trackers/selezen.ico)

Selezen (slug `selezen`) - релиз-группа с сайтом на DLE, около 16 тыс. раздач. CrabIndex читает список релизов `/relizy-ot-selezen/` (и `/relizy-ot-selezen/page/<n>/`) и для каждой раздачи на странице открывает страницу релиза, откуда берёт магнет-ссылку. Сайт доступен только авторизованным пользователям.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Selezen` |
| `host` | `https://use.selezen.club` (по умолчанию в коде); в `Data/example.yaml` указан `https://open.selezen.org` |
| `alias` | не используется |
| `reqMinute` | `8` → `parseDelay` 7000 мс (пауза между страницами списка) |
| `useproxy` | `false` (поддерживается) |
| Авторизация | обязательна: `login.u` / `login.p` или `cookie` (с `cookie` `login.u` всё равно нужен, см. ниже) |
| Кодировка | UTF-8 |
| Магнет-ссылки | со страницы релиза |
| Учитывает `disable_trackers` | да - `parse` возвращает `disabled` |

## Конфигурация

```yaml
Selezen:
  host: https://open.selezen.org   # актуальное зеркало; по умолчанию https://use.selezen.club
  useproxy: false
  reqMinute: 8
  log: true
  # cookie: "PHPSESSID=...;"       # необязательно: статическая сессия вместо входа
  login:
    u: "your_login"
    p: "your_password"
```

- **Логин и пароль (рекомендуется).** CrabIndex отправляет форму входа DLE на `host`, берёт `PHPSESSID` и хранит сессию в памяти 24 часа. Повторная попытка входа - не чаще раза в 2 минуты.
- **Статический cookie.** Если задан `cookie`, вход не выполняется. Скопируйте `PHPSESSID` из DevTools (F12 → Application → Cookies) после входа на сайт.

:::warning[Внимание]
Страница списка считается корректной, только если в ней видно имя пользователя из `login.u`. Поэтому `login.u` нужно указать даже при использовании `cookie`, иначе каждая страница завершится ошибкой `login not found in response`.
:::

При смене домена обновите `host`: ссылки на раздачи строятся от него.

## Cron-маршруты

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/selezen/parse` | `parseFrom` (0), `parseTo` (0) | `ok`, `work` или `disabled` | Синхронно разбирает страницы списка с `parseFrom` по `parseTo` включительно. Без параметров - только первая страница; только `parseFrom` - одна эта страница; перепутанные границы меняются местами |

Маршрут принимает `GET` и `POST`, путь и имена параметров регистронезависимы. Итоги (`parsed`, `added`, `updated`, `skipped`, `failed`) пишутся в лог трекера, а не в ответ.

## Рекомендуемое расписание

```crontab
# Первая страница каждые 15 минут
9,24,39,54 * * * *  /opt/crabindex/Data/run-job.sh selezen-parse http://127.0.0.1:9117/cron/selezen/parse 900
```

Для первичного наполнения базы запустите вручную диапазон страниц, например `?parseFrom=1&parseTo=50`.

## Файлы в Data/temp

Selezen не хранит состояния в `Data/temp`.

## Особенности и ограничения

- Страница каждого релиза из списка запрашивается при каждом проходе (изменился ли магнет, выясняется только на ней), поэтому длинные диапазоны выполняются долго.
- Раздачи сопоставляются с уже сохранёнными и по URL, и по числовому id релиза - смена slug в адресе не создаёт дубликат.
- Если страница релиза не содержит магнет-ссылки, раздача не сохраняется (`failed` в логе).
- Запросы отправляются с минимальным набором заголовков: лишние заголовки провоцируют защиту сайта.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/selezen/parse"
curl "http://127.0.0.1:9117/cron/selezen/parse?parseFrom=1&parseTo=5"
tail -f Data/log/selezen.log
```

Типичные записи лога: `TakeLogin success`, `TakeLogin failed reason=credentials not configured`, `Page parse failed reason=login not found in response`, `Page completed page=… parsed=… added=…`.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Прокси](../configuration/proxy.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Решение проблем](../operations/troubleshooting.md)
