# AniDub

![](/img/trackers/anidub.ico)

AniDub (slug `anidub`, около 4 тысяч раздач) - трекер аниме-озвучки на движке DLE. Авторизация не нужна. CrabIndex читает страницы листинга (`/` и `/page/N/`), а магнет и размер получает из `.torrent` раздачи или со страницы релиза.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Anidub` |
| `host` | `https://tr.anidub.com` |
| `alias` | не используется |
| `reqMinute` | `8` (задержка `parseDelay` = 7000 мс) |
| Авторизация | не нужна |
| Кодировка | UTF-8 |
| Магнет-ссылки | из `.torrent` (ссылка из листинга) или со страницы релиза |
| Учитывает `disable_trackers` | нет |

## Конфигурация

```yaml
Anidub:
  host: https://tr.anidub.com
  useproxy: false
  reqMinute: 8
  log: true
```

Логин и cookie для AniDub не используются.

## Cron-маршруты

Путь и имена параметров нечувствительны к регистру. Маршрут принимает GET и POST.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/anidub/parse` | `parseFrom` (по умолчанию 0 → страница 1), `parseTo` (по умолчанию 0 → равно `parseFrom`) | `ok`, `work` | Страницы листинга в диапазоне. Нумерация с 1; если `parseFrom > parseTo`, границы меняются местами |

## Рекомендуемое расписание

```crontab
# anidub-parse -> /cron/anidub/parse (max_time=900s)
4,19,34,49 * * * *  /opt/crabindex/Data/run-job.sh anidub-parse http://127.0.0.1:9117/cron/anidub/parse 900
```

Каждые 15 минут разбирается первая страница. Для первичного наполнения запустите вручную с диапазоном, например `?parseFrom=1&parseTo=100`.

## Файлы в Data/temp

Нет.

## Особенности и ограничения

- Для раздачи, уже сохранённой с тем же заголовком и магнетом, CrabIndex открывает страницу релиза и сравнивает магнет: если он не изменился, запись пропускается (но год выпуска дозаполняется, если его не было).
- Для новой раздачи сначала скачивается `.torrent` по ссылке из листинга; если не удалось - магнет берётся со страницы релиза, затем из `.torrent` по ссылке `engine/download.php` оттуда же.
- Скачивание `.torrent` по ссылке из листинга всегда идёт напрямую, без прокси, даже при `useproxy: true`.
- Страница листинга считается корректной только при наличии блока `dle-content`.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/anidub/parse"
curl "http://127.0.0.1:9117/cron/anidub/parse?parseFrom=1&parseTo=5"
tail -f Data/log/anidub.log
```

## См. также

- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
