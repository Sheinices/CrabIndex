# Kinozal

![](/img/trackers/kinozal.ico)

Kinozal (slug `kinozal`) - один из крупнейших трекеров в базе, около 550 тысяч раздач. Для чтения листингов нужна учётная запись (cookie или логин/пароль), сайт отдаёт HTML в кодировке cp1251 и закрыт Cloudflare. CrabIndex читает листинги `browse.php`, а info-hash каждой новой или изменившейся раздачи получает отдельным запросом `get_srv_details.php?id=<id>&action=2` и собирает из него magnet.

Kinozal - trio-трекер, но с вложенной картой задач: архив обходится по категориям и годам (`browse.php?c=<cat>&d=<год>&t=1&page=<N>`).

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Kinozal` |
| `host` | `https://kinozal.guru` |
| `alias` | не используется - все запросы идут на `host` |
| `reqMinute` | `8` (пауза между страницами полного обхода - 7000 мс) |
| Авторизация | обязательна: `cookie` или `login.u`/`login.p` |
| Кодировка | windows-1251 |
| Магнет-ссылки | info-hash из `get_srv_details.php` (отдельный запрос на раздачу) |
| Cloudflare | да, нужен FlareSolverr (и желательно cffetch) |
| Учитывает `disable_trackers` в cron | нет |

## Конфигурация

Статический cookie (предпочтительно):

```yaml
Kinozal:
  host: https://kinozal.guru
  useproxy: false
  reqMinute: 8
  log: true
  cookie: "uid=12345678; pass=XXXXXXXX;"
```

Логин и пароль:

```yaml
Kinozal:
  host: https://kinozal.guru
  useproxy: false
  reqMinute: 8
  log: true
  login:
    u: "mylogin"
    p: "mypassword"
```

Если задан `cookie`, логин не выполняется. Без cookie CrabIndex отправляет `POST <host>/takelogin.php` и берёт из ответа cookie `uid` и `pass`; полученный cookie хранится только в памяти и переполучается после рестарта или когда сайт снова показывает форму входа.

### Как получить cookie

1. Войдите в аккаунт на текущем домене Kinozal в браузере.
2. Откройте инструменты разработчика (F12) → Application (Storage) → Cookies.
3. Скопируйте значения `uid` и `pass`.
4. Запишите их в формате `uid=ЗНАЧЕНИЕ; pass=ЗНАЧЕНИЕ;`.

:::note[Примечание]
Cookie перестаёт действовать после выхода из аккаунта или смены пароля. Если парсинг вдруг перестал находить раздачи, первым делом обновите cookie.
:::

### Cloudflare

`kinozal.guru` отвечает `403` с заголовком `cf-mitigated: challenge`, поэтому прямые запросы не проходят. При таком ответе HTTP-клиент CrabIndex автоматически переключается на решатель Cloudflare: включите `flaresolverr.enable: true`, а для быстрых повторных запросов после решения - `cffetch` (`ghcr.io/jacred-fdb/cffetch`). Kinozal получает собственную сессию браузера `crabindex-kinozal_guru`. Настройка - [FlareSolverr и cffetch](../configuration/flaresolverr.md).

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Доступ - из локальной сети или с `devkey`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/kinozal/parse` | `page` (по умолчанию `0`) | лог `<cat> - <page>`; `work` | Синхронно разбирает страницу `page` всех категорий без фильтра по году (`browse.php?c=<cat>&page=<N>`) |
| `/cron/kinozal/UpdateTasksParse` | - | `ok`, `work` или `login failed: <причина>` | Фоновое построение карты: каждая категория × каждый год от текущего до 1990 |
| `/cron/kinozal/ParseAllTask` | - | `ok` или `work` | Фоновый полный обход карты (категория → год → страницы) |
| `/cron/kinozal/ParseLatest` | `pages` (по умолчанию `5`) | строки `<cat> - &d=<год>&t=1 - <page>`, `ok` или `work` | Первые `pages` страниц каждой пары категория/год из карты |

`UpdateTasksParse` сначала проверяет вход: без рабочего cookie или логина он сразу отвечает `login failed: …` и в фон не уходит.

## Рекомендуемое расписание

Из `Data/crontab`:

```crontab
# свежие раздачи каждые 15 минут
3,18,33,48 * * * *  /opt/crabindex/Data/run-job.sh kinozal-parse http://127.0.0.1:9117/cron/kinozal/parse 900

# обновление карты раз в сутки
10 2 * * *  /opt/crabindex/Data/run-job.sh kinozal-UpdateTasksParse http://127.0.0.1:9117/cron/kinozal/UpdateTasksParse 60

# полный обход (~4 ч) дважды в сутки
35 5,17 * * *  /opt/crabindex/Data/run-job.sh kinozal-ParseAllTask http://127.0.0.1:9117/cron/kinozal/ParseAllTask 60
```

`ParseAllTask` начинает новый круг только при pending = 0; после рестарта цикл продолжает `/cron/maintenance/ResumeParseAll`.

## Файлы в Data/temp

| Файл | Содержимое |
| --- | --- |
| `Data/temp/kinozal_taskParse.json` | Вложенная карта: категория → `&d=<год>&t=1` → страницы |
| `Data/temp/kinozal_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- **Часовой parse не блокируется архивом.** `parse` идёт под своей блокировкой, а `ParseAllTask`/`ParseLatest` уступают ему между страницами. `UpdateTasksParse` и `ParseAllTask` взаимоисключающие (общий флаг): пока идёт один, второй отвечает `work`.
- **Лимит `UpdateTasksParse` - 2 часа** (а не 30 минут, как у остальных трекеров): карта строится для всех категорий × ~37 лет. Пауза между запросами здесь равна `parseDelay`, но не больше 2 с.
- **Число страниц года.** Цифра в пейджере перед `rel="next"` - номер последней страницы (с единицы), а `page` в URL считается с нуля: при цифре `15` в карту попадают страницы `0..14`. Без пейджера - одна страница. Слоты с `page >= числа страниц` удаляются.
- **Пустой год.** Ответ «Нет активных раздач» - настоящая пустая выдача: страница считается пройденной. Для `page=0` CrabIndex перед этим дважды перезапрашивает страницу с паузой 3 с.
- **Устаревшая вкладка браузера.** «Залогиненная» страница без таблицы раздач (`t_peer`), а также листинг, у которого выбранные в форме категория или год не совпадают с запрошенными, считаются остатком предыдущей вкладки FlareSolverr. Страница перезапрашивается до 3 раз с паузой 3 с и **не** отмечается пройденной; после трёх таких ответов подряд сессия FlareSolverr для хоста пересоздаётся.
- **Хеши.** Info-hash перезапрашивается, если у раздачи изменились заголовок, размер или дата в листинге (Kinozal перехеширует `.torrent` при добавлении серий). Страница засчитывается пройденной, только когда magnet получен для всех её раздач.
- **Ссылки** в FileDB - только `<host>/details.php?id=<id>`; ссылки на профили (`userdetails.php`) игнорируются.
- Полный проход медленнее, чем у трекеров с magnet в листинге: на каждую новую раздачу - отдельный запрос хеша. Процент `ParseAllTask` в `/health/background-jobs` - доля обойдённых страниц очереди, а не число новых раздач; итог смотрите в логе (`ParseAllTask ok=успешно/попыток`).

## Диагностика

```bash
# ручной parse
curl "http://127.0.0.1:9117/cron/kinozal/parse"

# прогресс фоновых задач
curl "http://127.0.0.1:9117/health/background-jobs"

# лог парсера
tail -f /opt/crabindex/Data/log/kinozal.log
```

Полезные строки лога:

| Строка | Значение |
| --- | --- |
| `TakeLogin failed: credentials not configured …` | Не заданы ни `cookie`, ни `login.u`/`login.p` |
| `TakeLogin failed: no uid/pass cookies in response` | Сайт не принял логин/пароль |
| `browse empty search` | Пустая выдача года - страница пройдена |
| `browse stale/empty shell`, `browse filter mismatch` | Остаток старой вкладки - страница будет повторена |
| `recycle FlareSolverr session after consecutive stale shells` | Сессия браузера пересоздана |

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [FlareSolverr и cffetch](../configuration/flaresolverr.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Решение проблем](../operations/troubleshooting.md)
