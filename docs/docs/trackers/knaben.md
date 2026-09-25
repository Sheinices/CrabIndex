# Knaben

![](/img/trackers/knaben.ico)

Knaben (slug `knaben`) - метапоисковик с публичным JSON API (`POST {host}/v1`), авторизация не нужна. CrabIndex забирает из него категории TV (`2000000`-`2008000`) и Movies (`3000000`-`3008000`), которые Knaben собирает с TPB, 1337x, EZTV, Rutracker и других источников. В базе обычно около 100 тысяч раздач. HTML не парсится, запросы идут только к API.

Задач две. `parse` берёт свежие раздачи. `backfill` постепенно выкачивает архив по листовым подкатегориям, в пределах окна API в 10 000 записей. Курсор между запусками сохраняется.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Knaben` |
| `host` | `https://api.knaben.org` |
| `alias` | не используется (запросы всегда идут на `host`) |
| `reqMinute` | `8` → `parseDelay` 7000 мс; в `Data/example.yaml` - `120` → 1000 мс |
| Авторизация | не нужна |
| Кодировка | JSON, UTF-8 |
| Магнет-ссылки | из API; если магнета нет, он строится из `.torrent` по ссылке |
| Учитывает `disable_trackers` | нет |

Перед каждым запросом к API выдерживается пауза `max(500 мс, parseDelay)`.

## Конфигурация

```yaml
Knaben:
  host: https://api.knaben.org
  useproxy: false
  reqMinute: 120   # быстрый API - можно чаще стандартных 8 запросов в минуту
  log: true
```

## Cron-маршруты

Регистр в путях и именах параметров не важен (`orderBy` = `orderby`).

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/knaben/parse` | см. таблицу ниже | `fetched=N +added ~updated =skipped failed=N`; `work`; `canceled`; `error: …` | Синхронная выборка свежих раздач |
| `/cron/knaben/backfill` | `size` (`300`, 1-300), `pages` (`10`, 1-10), `reset` (`false`) | `cat=… dir=… from=… fetched=… +… ~… =… failed=… progress=X/16[ partial=N][ finished]`; `… (finished)`, если архив уже пройден; `work`; `canceled`; `error: …` | Следующие `pages` страниц архивного прохода |
| `/cron/knaben/BackfillStatus` | - | `finished=… cat=… dir=… from=… totalFetched=… +… ~… progress=X/16 updatedAt=…` | Состояние курсора backfill без запросов к API |

Параметры `parse`:

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `from` | `0` | Смещение. Должно быть < 10000, иначе ответ `error: from must be < 10000 …` |
| `size` | `300` | Размер страницы, 1-300 |
| `pages` | `1` | Сколько страниц подряд, 1-10. Выборка останавливается раньше, если страница неполная или упёрлась в окно 10 000 |
| `query` | - | Поиск по заголовку |
| `hours` | `0` | Если > 0 - только раздачи, замеченные за последние N часов |
| `orderBy` | `date` | `date`, `seeders` или `peers` (иное значение → `date`) |
| `orderDirection` | `desc` | `asc` или `desc` |
| `categories` | 18 категорий TV + Movies | Список ID через запятую, `;` или пробел |

`parse` и `backfill` держат разные блокировки, поэтому могут работать одновременно.

## Как работает backfill

- Обходятся 16 листовых подкатегорий (`2001000`-`2008000`, `3001000`-`3008000`), без родительских `2000000`/`3000000`.
- Внутри подкатегории сначала идёт проход `asc` (от старых к новым). Если лента кончилась раньше окна, подкатегория помечается `complete`. Если проход упёрся в окно 10 000, начинается проход `desc`. Когда `desc` пересекается с краем `asc`, статус - `complete`, иначе `partial`: середина больше окна API и недоступна.
- Если страница пришла неполной или битой, её повторяют до 3 раз с паузами 2 и 8 с. Если попытки кончились, курсор остаётся на месте, и следующий вызов начнёт с той же страницы.
- Когда все подкатегории пройдены, `finished=True`, и дальнейшие вызовы ничего не делают. Новый проход запускается через `?reset=true`.

## Рекомендуемое расписание

В `Data/crontab` обе строки Knaben **закомментированы**, то есть по умолчанию трекер не опрашивается. Чтобы включить его, раскомментируйте:

```crontab
# knaben-parse - свежие раздачи
12,32,52 * * * *  /opt/crabindex/Data/run-job.sh knaben-parse http://127.0.0.1:9117/cron/knaben/parse 900

# knaben-backfill - архив листовых подкатегорий TV/Movies, курсор в Data/temp/knaben_backfill.json
42 * * * *  /opt/crabindex/Data/run-job.sh knaben-backfill "http://127.0.0.1:9117/cron/knaben/backfill?pages=10" 900
```

Раздачи Knaben можно получать и без собственного опроса, через синхронизацию с `https://sync.crab.rip`, если `knaben` есть в `synctrackers` (см. [Синхронизация](../concepts/sync.md)).

## Файлы в Data/temp

| Файл | Назначение |
| --- | --- |
| `Data/temp/knaben_backfill.json` | Курсор backfill: подкатегория, направление, смещение, статусы подкатегорий, итоговые счётчики |

## Особенности и ограничения

- API отдаёт не больше 10 000 записей на запрос с учётом смещения (`from + size ≤ 10000`). Поэтому очень большие подкатегории проходятся с двух концов и могут остаться `partial`.
- URL раздачи в FileDB - ссылка на страницу первоисточника из API. Если её нет, используется `link`, а в крайнем случае `https://knaben.xyz/?id=…`. К заголовку добавляется `| <источник>`, если его там ещё нет.
- Скрываются небезопасные и XXX-раздачи (`hideUnsafe`, `hideXXX`).
- Имя и год разбираются из заголовка. Для уже сохранённых записей их можно пересчитать через `/dev/FixKnabenNames` (см. [Dev API](../api-reference/dev.md)).

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/knaben/parse?hours=6&pages=2"
curl "http://127.0.0.1:9117/cron/knaben/backfill?pages=1"
curl "http://127.0.0.1:9117/cron/knaben/BackfillStatus"
cat Data/temp/knaben_backfill.json
tail -f Data/log/knaben.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Обслуживание FileDB](../operations/maintenance.md)
