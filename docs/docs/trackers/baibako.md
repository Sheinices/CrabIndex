# BaibaKo

![](/img/trackers/baibako.ico)

BaibaKo (slug `baibako`, около 5 тысяч раздач) - трекер сериалов. CrabIndex авторизуется (cookie или логин/пароль), читает страницы `browse.php?page=N` и скачивает `.torrent` каждой раздачи, чтобы получить магнет и размер.

:::note[Примечание]
Сериальный каталог BaibaKo переехал на RuDub. Для свежих релизов используйте трекер [RuDub](rudub.md) (slug `rudub`). Маршрут `baibako` оставлен в CrabIndex, но в стандартном `Data/crontab` его нет: записи `baibako` в базе продолжают отдаваться в поиске и синхронизации, а новые загружаются только ручным запуском. Не переименовывайте `baibako` в `rudub` - это разные slug с разными записями в FileDB.
:::

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Baibako` |
| `host` | `http://baibako.tv` |
| `alias` | не используется |
| `reqMinute` | `8` (задержка `parseDelay` = 7000 мс) |
| Авторизация | обязательна: `cookie` или `login.u` / `login.p` |
| Кодировка | Windows-1251 (перекодируется автоматически) |
| Магнет-ссылки | из скачанного `.torrent` |
| Учитывает `disable_trackers` | нет; пустой `host` отключает маршрут (ответ `disabled`) |

## Конфигурация

Вариант с cookie:

```yaml
Baibako:
  host: http://baibako.tv
  reqMinute: 8
  log: true
  cookie: "PHPSESSID=СЕССИЯ; uid=ID_ПОЛЬЗОВАТЕЛЯ; pass=ХЕШ"
```

Вариант с логином и паролем:

```yaml
Baibako:
  host: http://baibako.tv
  login:
    u: "mylogin"
    p: "mypassword"
```

Если `cookie` задан, он используется всегда. Иначе CrabIndex выполняет вход через `POST /takelogin.php` и держит полученные `PHPSESSID`, `uid` и `pass` в памяти 24 часа. Без cookie и без логина маршрут отвечает `login error`.

### Как получить cookie

1. Войдите в аккаунт на сайте в браузере.
2. DevTools (F12) → Application → Cookies.
3. Скопируйте `PHPSESSID`, `uid` и `pass` и соберите строку `PHPSESSID=…; uid=…; pass=…`.

## Cron-маршруты

Пути и имена параметров нечувствительны к регистру. Маршрут принимает GET и POST.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/baibako/parse` | `parseFrom` (по умолчанию 0, отрицательное → 0), `parseTo` (по умолчанию 0; отрицательное → равно `parseFrom`) | `ok`, `work`, `login error`, `disabled` | Страницы `browse.php?page=N` в диапазоне. Нумерация с нуля; если `parseFrom > parseTo`, границы меняются местами |

Без параметров разбирается только страница 0 - самые новые раздачи. Обратите внимание: `parseTo` по умолчанию равен 0, поэтому `?parseFrom=5` без `parseTo` разберёт страницы 0-5.

## Рекомендуемое расписание

В стандартном `Data/crontab` задачи для `baibako` нет. Если нужно периодически обновлять старый каталог, добавьте строку вручную, например раз в сутки:

```crontab
# baibako-parse -> /cron/baibako/parse (max_time=900s)
30 6 * * *  /opt/crabindex/Data/run-job.sh baibako-parse http://127.0.0.1:9117/cron/baibako/parse 900
```

## Файлы в Data/temp

Нет. Сессия после входа хранится только в памяти.

## Особенности и ограничения

- Для новой раздачи всегда скачивается `.torrent`; для существующей с тем же заголовком - тоже, чтобы сравнить магнет и размер. Если ничего не изменилось, запись пропускается.
- Если скачивание возвращает HTML вместо торрента, раздача отмечается как ошибочная (обычно это значит, что сессия протухла).
- Ключ `useproxy` для этого трекера не действует: запросы идут напрямую.
- Тип (`serial` или `movie`) определяется по заголовку.
- Страница считается корректной только при наличии блока навигации `id="navtop"`; иначе в логе появится `Page parse failed … invalid content`.

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/baibako/parse"
curl "http://127.0.0.1:9117/cron/baibako/parse?parseFrom=0&parseTo=10"
tail -f Data/log/baibako.log
```

## См. также

- [RuDub](rudub.md)
- [Обзор трекеров](overview.mdx)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
