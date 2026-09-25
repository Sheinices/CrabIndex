# AniLiberty

![](/img/trackers/aniliberty.ico)

AniLiberty (slug `aniliberty`) - аниме-релизы, около 5 тысяч раздач. CrabIndex читает официальный JSON API `GET {host}/api/v1/anime/torrents?page=N&limit=50`, по 50 торрентов на страницу. Авторизация не нужна: магнет-ссылка, сиды, пиры, размер и качество приходят прямо в ответе API.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Aniliberty` |
| `host` | `https://aniliberty.top` |
| `alias` | не используется: запросы всегда идут на `host` |
| `reqMinute` | `8` → задержка `parseDelay` 7000 мс между страницами API |
| Авторизация | нет |
| Кодировка | UTF-8 (JSON) |
| Магнет-ссылки | из ответа API |
| Учитывает `disable_trackers` | нет, маршрут выполняется и для отключённого трекера |

## Конфигурация

```yaml
Aniliberty:
  host: https://aniliberty.top
  useproxy: false
  reqMinute: 8
  log: true
```

Логин и cookie не нужны.

## Cron-маршруты

Путь и имена параметров не зависят от регистра. Маршрут принимает только `GET`.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/aniliberty/parse` | `parseFrom` (по умолчанию `1`), `parseTo` (по умолчанию равен `parseFrom`) | `ok` после завершения, `work` если parse уже идёт | Обходит страницы API с `parseFrom` по `parseTo` включительно |

Правила диапазона:

- `parseFrom` ≤ 0 считается страницей 1.
- `parseTo` ≤ 0 равен `parseFrom`, поэтому без параметров обходится только первая, самая свежая страница.
- Если `parseFrom` больше `parseTo`, границы меняются местами.
- Обход останавливается на последней странице из `meta.last_page` ответа API, даже если `parseTo` больше.

Для первичного заполнения базы задайте широкий диапазон, например `?parseFrom=1&parseTo=200`. Лишние страницы будут отброшены по `last_page`. Учтите, что запрос синхронный и займёт `parseDelay` × число страниц.

## Рекомендуемое расписание

```crontab
6,21,36,51 * * * *  /opt/crabindex/Data/run-job.sh aniliberty-parse http://127.0.0.1:9117/cron/aniliberty/parse 900
```

Раз в 15 минут считывается первая страница API. Этого хватает, чтобы подхватывать новые релизы.

## Файлы в Data/temp

Нет. Трекер не хранит карту страниц и не участвует в `ParseAllTask`.

## Особенности и ограничения

- Ссылка раздачи в FileDB: `{host}/anime/releases/release/{alias релиза}?hash={infohash}`. Если у релиза нет alias, используется `{host}/api/v1/anime/torrents/{hash}?hash={hash}`. Параметр `hash` делает ссылку уникальной для каждого торрента релиза.
- Заголовок собирается как `Русское / English / год / качество`. Типы определяются по полю типа релиза: `MOVIE` → `anime`+`movie`, `OVA`/`OAD` → `ova`, `ONA`/`WEB` → `ona`, `SPECIAL` → `special`, `DORAMA` → `dorama`, остальное → `anime`+`serial`.
- Запись обновляется, только если изменилась магнет-ссылка. Иначе она пропускается как `no changes`.
- Для баз, заполненных до появления `?hash=` в ссылках, есть служебные маршруты [`/dev/MigrateAnilibertyUrls`](../api-reference/dev.md) (дописывает `hash=` к старым ссылкам) и `/dev/RemoveDuplicateAniliberty` (оставляет по одной, самой свежей записи на infohash).

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/aniliberty/parse"
curl "http://127.0.0.1:9117/cron/aniliberty/parse?parseFrom=1&parseTo=5"
tail -f /opt/crabindex/Data/log/aniliberty.log
```

В логе строки `Page parse failed … reason=null response` означают, что API недоступен. Проверьте `host` и [прокси](../configuration/proxy.md).

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Обслуживание FileDB](../operations/maintenance.md)
