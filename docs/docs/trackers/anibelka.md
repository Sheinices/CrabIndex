# Anibelka

![](/img/trackers/anibelka.ico)

Anibelka (slug `anibelka`) - аниме-трекер на движке phpBB. CrabIndex обходит пять разделов «Скачать аниме» (`f=32` универсальные, `33` с озвучкой, `34` с субтитрами, `36` полнометражки, `37` PSP) и сохраняет все раздачи с типом `anime`.

Трекер работает **только анонимно**. Для каждой темы из листинга CrabIndex открывает страницу темы (сиды, пиры, размер, дата), а для новых или изменённых раздач скачивает `.torrent` через `/download/file.php?id=…` и строит из него магнет-ссылку. Анонимный `.torrent` не содержит персонального passkey, поэтому магнет безопасно отдавать клиентам. Если бы запросы шли с cookie аккаунта, passkey попал бы в магнет и стал публичным через API. Поэтому парсер не отправляет ни cookie, ни логин, даже если они заданы в конфиге.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Anibelka` |
| `host` | `https://anibelka.com` |
| `alias` | не задан |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс |
| Авторизация | нет, только анонимный доступ |
| Кодировка | UTF-8 |
| Магнет-ссылки | из анонимного `.torrent` (скачивается только для новых раздач, при смене заголовка или пустом магнете) |
| Учитывает `disable_trackers` | да, все четыре маршрута возвращают `disabled` |

## Конфигурация

```yaml
Anibelka:
  host: https://anibelka.com
  useproxy: false
  reqMinute: 8
  log: true
  # cookie и login не задавайте: парсер их не использует
```

:::danger[Предупреждение]
Не добавляйте в блок `Anibelka` поля `cookie` и `login`. Анонимный режим нужен, чтобы в базу не попал ваш passkey.
:::

`parseDelay` вычисляется из `reqMinute` (60 / `reqMinute` секунд) и задавать его вручную не нужно. Если задан `alias`, запросы идут на него, и ссылки на темы в FileDB тоже строятся от `alias`.

## Cron-маршруты

Пути и имена параметров не зависят от регистра. Все маршруты принимают `GET` и `POST`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/anibelka/parse` | `page` (по умолчанию `0`) - номер страницы листинга, с нуля | текстовый лог `раздел - страница` по строке на раздел, `work` если parse уже идёт | Синхронно обходит одну страницу каждого из пяти разделов |
| `/cron/anibelka/UpdateTasksParse` | нет | `ok` / `work` / `disabled` сразу | В фоне читает пейджер первой страницы каждого раздела и обновляет карту страниц |
| `/cron/anibelka/ParseAllTask` | нет | `ok` / `work` / `disabled` сразу | В фоне обходит все страницы из карты в рамках цикла |
| `/cron/anibelka/ParseLatest` | `pages` (по умолчанию `5`, значение ≤ 0 тоже даёт 5) | лог обойдённых страниц или `ok`, `work` при конфликте | Первые `pages` страниц каждого раздела из карты |

Если `host` пуст, `parse` и `ParseLatest` возвращают `config missing`.

Поведение фоновых задач:

- `ParseAllTask` начинает новый круг только когда в текущем цикле не осталось необработанных страниц (pending = 0). Незавершённый цикл после рестарта продолжает `/cron/maintenance/ResumeParseAll`. Если 45 минут нет прогресса, задачу останавливает сторож.
- `UpdateTasksParse` ограничен 30 минутами.
- `ParseLatest`, `ParseAllTask` и `UpdateTasksParse` взаимоисключающие. Часовой `parse` не блокируется: фоновый обход уступает ему между страницами.
- Если карта страниц пуста, `ParseAllTask` и `ParseLatest` сначала строят её сами.
- В `ParseAllTask` пустой листинг тоже считается обработанной страницей.

## Рекомендуемое расписание

```crontab
# каждые 30 минут (parse занимает около 5 минут)
29,59 * * * *  /opt/crabindex/Data/run-job.sh anibelka-parse http://127.0.0.1:9117/cron/anibelka/parse 900

# обновление карты страниц раз в сутки
40 2 * * *  /opt/crabindex/Data/run-job.sh anibelka-UpdateTasksParse http://127.0.0.1:9117/cron/anibelka/UpdateTasksParse 60

# полный обход (~3,7 ч); новый круг только при pending = 0
48 5,17 * * *  /opt/crabindex/Data/run-job.sh anibelka-ParseAllTask http://127.0.0.1:9117/cron/anibelka/ParseAllTask 60

# свежие страницы раз в сутки
10 7 * * *  /opt/crabindex/Data/run-job.sh anibelka-ParseLatest http://127.0.0.1:9117/cron/anibelka/ParseLatest 900
```

Продолжение прерванных циклов обеспечивает общая задача `parseall-resume` (`*/15`, `/cron/maintenance/ResumeParseAll`).

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/anibelka_taskParse.json` | Карта страниц: раздел → список страниц с отметками обхода |
| `Data/temp/anibelka_parseAllCycle.json` | Состояние текущего цикла `ParseAllTask` |

## Особенности и ограничения

- На странице листинга 15 тем. Последняя страница берётся из самой большой ссылки пейджера `viewforum.php?f=…&start=N`. Числа `start=` из скриптов не учитываются. Если пейджер сжался, лишние страницы удаляются из карты. Пустой ответ карту не обнуляет.
- Для каждой темы всегда запрашивается её страница, а перед каждым запросом выдерживается `parseDelay`. Поэтому обход медленный: около 5 минут на одну страницу всех разделов при значениях по умолчанию.
- `.torrent` скачивается повторно, только если раздача новая, её заголовок изменился или в базе пустой магнет. Раздачи без магнета в базу не попадают.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/anibelka/parse"
curl "http://127.0.0.1:9117/cron/anibelka/ParseLatest?pages=2"
curl "http://127.0.0.1:9117/cron/maintenance/ParseAllStatus"
curl "http://127.0.0.1:9117/health/background-jobs"
tail -f /opt/crabindex/Data/log/anibelka.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
