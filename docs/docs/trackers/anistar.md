# AniStar

![](/img/trackers/anistar.ico)

AniStar (slug `anistar`) - аниме-трекер на движке DLE, который часто меняет зеркала. CrabIndex обходит три раздела - `anime`, `hentai` (оба с типом `anime`) и `dorams` (тип `serial`), - открывает страницу каждого релиза и скачивает `.torrent` через `engine/gettorrent.php`.

Трекер использует двухуровневую схему адресов: `host` - канонический адрес, под которым URL раздач хранятся в FileDB, а `alias` - живое зеркало, на которое реально уходят запросы. При смене зеркала меняйте только `alias`.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Anistar` |
| `host` | `https://anistar.org` |
| `alias` | не задан (в `Data/example.yaml` - `https://v30.astar.bz`) |
| `reqMinute` | `8` (задержка `parseDelay` = 7000 мс, пауза после каждого релиза) |
| Авторизация | не нужна, `cookie` необязателен |
| Кодировка | Windows-1251 (перекодируется автоматически) |
| Магнет-ссылки | из `.torrent` (`engine/gettorrent.php?id=…`) |
| Учитывает `disable_trackers` | нет |

## Конфигурация

```yaml
Anistar:
  host: https://anistar.org      # канонический адрес для URL в FileDB - не меняйте
  alias: https://v30.astar.bz    # текущее рабочее зеркало для запросов
  useproxy: false
  reqMinute: 8
  log: true
  cookie: ""                     # необязательно
```

:::warning[Внимание]
Не меняйте `host` при переезде сайта. Ссылки на страницах зеркала переписываются на `host`, и под этими URL раздачи лежат в FileDB. Смена `host` приведёт к появлению дублей вместо обновления существующих записей.
:::

## Cron-маршруты

Путь и имена параметров нечувствительны к регистру. Маршрут принимает GET и POST.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/anistar/parse` | `limit_page` (по умолчанию 0) | `ok`, `empty`, `work`, `config missing` | Обходит страницы 1…`limit_page` каждого из трёх разделов |

Если `limit_page` ≤ 0 (или не указан), последняя страница определяется по пейджеру (`div.pages`) каждого раздела, то есть выполняется **полный обход** - это долго. Для регулярного запуска всегда передавайте небольшое значение.

Ответы:

- `ok` - найдена хотя бы одна раздача;
- `empty` - ни на одной странице не удалось извлечь релизы (обычно зеркало недоступно или отдаёт Cloudflare challenge);
- `config missing` - пустой `host` или адрес запросов.

## Рекомендуемое расписание

```crontab
# anistar-parse -> /cron/anistar/parse?limit_page=3 (max_time=900s)
40 6 * * *  /opt/crabindex/Data/run-job.sh anistar-parse "http://127.0.0.1:9117/cron/anistar/parse?limit_page=3" 900
```

Раз в сутки проходятся первые три страницы каждого раздела. Для первичного наполнения базы запустите вручную с большим `limit_page` (например, 20-30) или без него.

## Файлы в Data/temp

Нет.

## Особенности и ограничения

- Релиз, уже сохранённый с тем же заголовком и непустым магнетом, пропускается без скачивания `.torrent`.
- Ответ с Cloudflare challenge считается неудачным: страница листинга пропускается, релиз помечается как ошибочный. Обход Cloudflare для AniStar не выполняется - выберите зеркало без challenge.
- Для каждого релиза - отдельный запрос страницы и скачивание торрента, плюс пауза `parseDelay`, поэтому полный обход занимает часы.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/anistar/parse?limit_page=1"
tail -f Data/log/anistar.log
```

В логе ищите `Page fetch failed` с `reason` = `cloudflare challenge` или `null response` - это признак того, что `alias` пора сменить.

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
