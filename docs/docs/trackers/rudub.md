# RuDub

![](/img/trackers/rudub.ico)

RuDub (slug `rudub`) - закрытый трекер сериалов и фильмов, преемник BaibaKoTV. Нужна авторизация: cookie или логин с паролем. CrabIndex читает `browse.php` только с фильтрами HD 1080 (`videoformat=4`) и HD 2160 (`videoformat=5`). Из заголовков дополнительно отбрасываются XviD, SD x264 и 720p.

Для каждой раздачи CrabIndex скачивает `.torrent` по сессии пользователя и превращает его в магнет **без трекеров**. Так passkey не попадает в FileDB, а клиент находит пиров через DHT/PEX. Задач ParseAll здесь нет: есть один `parse`, который проходит заданный диапазон страниц.

## Параметры по умолчанию

| Параметр | Значение |
| --- | --- |
| Блок в `init.yaml` | `Rudub` |
| `host` | `https://r4.rudub.world` |
| `alias` | не задан |
| `reqMinute` | `8` → пауза `parseDelay` 7000 мс между страницами листинга |
| Авторизация | `cookie` или `login.u` / `login.p` |
| Кодировка | Windows-1251 (перекодируется автоматически) |
| Магнет-ссылки | из скачанного `.torrent`, без трекеров |
| Учитывает `disable_trackers` | нет (при пустом `host` маршрут отвечает `disabled`) |

## Конфигурация

Вариант с cookie:

```yaml
Rudub:
  host: https://r4.rudub.world
  alias: ""
  useproxy: false
  reqMinute: 8
  log: true
  cookie: "PHPSESSID=SESSION; uid=USER_ID; pass=PASS_HASH"
```

Вариант с логином:

```yaml
Rudub:
  host: https://r4.rudub.world
  useproxy: false
  reqMinute: 8
  log: true
  login:
    u: "mylogin"
    p: "mypassword"
```

Если задан `cookie`, логин не используется. Иначе CrabIndex отправляет `POST /takelogin.php` и собирает cookie из `PHPSESSID`, `uid` и `pass`. Полученная сессия хранится в памяти 24 часа. Если нет ни cookie, ни логина, `parse` вернёт `login error`.

Чтобы получить cookie, войдите на сайт, откройте DevTools → Application → Cookies и скопируйте `PHPSESSID`, `uid` и `pass` в формате `PHPSESSID=…; uid=…; pass=…`.

:::note[Примечание]
Зеркала RuDub меняются (`r1`, `r2`, `r3`, `r4`… `.rudub.world`). Если трекер перестал отвечать, поменяйте `host` на рабочее зеркало.
:::

## Cron-маршруты

Маршрут принимает только `GET`. Регистр в путях и именах параметров не важен.

| Маршрут | Параметры | Ответ | Описание |
| --- | --- | --- | --- |
| `/cron/rudub/parse` | `limit_page` (по умолчанию `0`), `parseFrom` (`0`), `parseTo` (`0`) | `ok`; `work`, если parse уже идёт; `login error`; `disabled` при пустом `host` | Синхронно проходит диапазон страниц для `videoformat` 4, затем 5 |

Как вычисляется диапазон страниц:

- `limit_page` > 0 при `parseFrom` = `parseTo` = 0: страницы `0 … limit_page-1`, `limit_page` ограничен диапазоном 1-100;
- иначе используются `parseFrom … parseTo` (отрицательный `parseFrom` считается как 0, отрицательный `parseTo` - как `parseFrom`, перевёрнутые границы меняются местами);
- без параметров разбирается только страница `0`.

Каждая страница проходится дважды, для 1080 и для 2160, поэтому `limit_page=10` даёт 20 запросов листинга плюс скачивание `.torrent`.

## Рекомендуемое расписание

```crontab
# каждый час в :40 - страницы 0..9 × videoformat 4+5
40 * * * *  /opt/crabindex/Data/run-job.sh rudub-parse "http://127.0.0.1:9117/cron/rudub/parse?limit_page=10" 1800
```

Минута `:40` выбрана так, чтобы не совпадать с другими почасовыми парсерами. Для первого заполнения базы вызовите вручную `?limit_page=50` или задайте диапазон через `parseFrom`/`parseTo`.

## Файлы в Data/temp

Нет. Состояния между запусками RuDub не хранит.

## Особенности и ограничения

- `.torrent` скачивается почти для каждой строки, в том числе для уже известной раздачи: так проверяется, не сменились ли магнет и размер. Исключение - строка, у которой изменились только типы (`serial`/`movie`): её обновляют без скачивания. Пауза `parseDelay` действует между страницами листинга, а не между скачиваниями.
- Если вместо `.torrent` пришёл HTML (протухла сессия, нет прав), раздача пропускается. В логе будет `downloaded HTML instead of torrent file`.
- Год (`relased`) берётся из `(YYYY)` в заголовке, а если его нет - из даты загрузки. Чтобы проставить год уже сохранённым раздачам без скачивания `.torrent`, есть `/dev/FixRudubRelased` (см. [Dev API](../api-reference/dev.md)).

## Диагностика

```bash
curl "http://127.0.0.1:9117/cron/rudub/parse?limit_page=2"
curl "http://127.0.0.1:9117/cron/rudub/parse?parseFrom=10&parseTo=19"
tail -f Data/log/rudub.log
```

`Login FAILED` в логе означает, что нет cookie или неверен пароль. `Page parse failed … invalid content` - страница пришла без карточек (сессия не авторизована или сменилось зеркало).

## См. также

- [Обзор трекеров](overview.mdx)
- [BaibaKo](baibako.md)
- [Настройка трекеров](../configuration/trackers.md)
- [Cron](../deployment/cron.md)
- [Cron API](../api-reference/cron.md)
- [Прокси](../configuration/proxy.md)
- [Решение проблем](../operations/troubleshooting.md)
