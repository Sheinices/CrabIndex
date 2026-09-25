# Трекеры

Каждый из 25 трекеров настраивается своим блоком в `init.yaml`. Блок необязателен: если его нет, используются значения по умолчанию. Имена блоков не чувствительны к регистру.

## Структура блока

```yaml
Kinozal:
  host: https://kinozal.guru      # канонический адрес трекера
  alias: ""                       # зеркало для запросов (необязательно)
  useproxy: false                 # ходить через пул proxy
  reqMinute: 8                    # лимит запросов в минуту
  log: true                       # писать Data/log/kinozal.log
  cookie: ""                      # готовая сессионная cookie
  login:
    u: ""                         # логин
    p: ""                         # пароль
```

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `host` | string | свой у каждого трекера | Базовый адрес. Из него строятся URL раздач, которые хранятся в FileDB |
| `alias` | string | пусто | Зеркало, onion-адрес или Worker. Если задан, **запросы** идут на `alias`, а URL в FileDB остаются на `host` - база не «переезжает» при смене зеркала |
| `useproxy` | bool | `false` | Направлять запросы через пул [`proxy`](proxy.md). Правила `globalproxy` действуют независимо от этого флага |
| `reqMinute` | int | `8` | Сколько запросов в минуту допускается; из него вычисляется пауза между страницами |
| `log` | bool | `true` | Журнал парсера `Data/log/{трекер}.log` (работает при `logParsers: true`) |
| `cookie` | string | пусто | Сессионная cookie, скопированная из браузера. Если задана, логин обычно не нужен |
| `login.u`, `login.p` | string | пусто | Логин и пароль для трекеров, которые умеют авторизоваться сами |
| `topicFetchAttempts` | int | `5` | Только Rutracker: число попыток загрузить страницу раздачи через FlareSolverr |

### reqMinute и пауза между запросами

Пауза между страницами вычисляется из `reqMinute`:

| `reqMinute` | Пауза |
| --- | --- |
| `-1` | 10 мс (без ограничения) |
| `0` или меньше | 60 секунд |
| от 1 до 59 | `60 / reqMinute` секунд (целочисленно): `8` → 7 с, `5` → 12 с, `30` → 2 с |
| `60` и больше | 1 секунда |

:::note[Примечание]
Ключ `parseDelay`, который встречается в `Data/example.yaml`, при загрузке игнорируется: пауза всегда вычисляется из `reqMinute`. Config API показывает `parseDelay` только для информации.
:::

Уменьшайте `reqMinute`, если трекер отвечает `429` или временно блокирует адрес.

## Блоки и адреса по умолчанию

| Блок | Трекер | `host` по умолчанию |
| --- | --- | --- |
| `Rutracker` | [Rutracker](../trackers/rutracker.md) | `https://rutracker.org` |
| `Kinozal` | [Kinozal](../trackers/kinozal.md) | `https://kinozal.guru` |
| `Rutor` | [Rutor](../trackers/rutor.md) | `http://rutor.info` |
| `NNMClub` | [NNM-Club](../trackers/nnmclub.md) | `https://nnmclub.to` |
| `Megapeer` | [Megapeer](../trackers/megapeer.md) | `http://megapeer.vip` |
| `Toloka` | [Toloka](../trackers/toloka.md) | `https://toloka.to` |
| `TorrentBy` | [Torrent.by](../trackers/torrentby.md) | `https://torrent.by` |
| `Anibelka` | [Anibelka](../trackers/anibelka.md) | `https://anibelka.com` |
| `Korsars` | [Korsars](../trackers/korsars.md) | `https://korsars.pro` |
| `Ultradox` | [Ultradox](../trackers/ultradox.md) | `https://ultradox.vip` |
| `Mazepa` | [Mazepa](../trackers/mazepa.md) | `https://mazepa.to` |
| `Lostfilm` | [LostFilm](../trackers/lostfilm.md) | `https://www.lostfilm.tv` |
| `Selezen` | [Selezen](../trackers/selezen.md) | `https://use.selezen.club` |
| `Baibako` | [BaibaKo](../trackers/baibako.md) | `http://baibako.tv` |
| `Animelayer` | [AnimeLayer](../trackers/animelayer.md) | `https://animelayer.ru` |
| `Anidub` | [AniDub](../trackers/anidub.md) | `https://tr.anidub.com` |
| `Aniliberty` | [AniLiberty](../trackers/aniliberty.md) | `https://aniliberty.top` |
| `Rudub` | [RuDub](../trackers/rudub.md) | `https://r4.rudub.world` |
| `Anistar` | [AniStar](../trackers/anistar.md) | `https://anistar.org` |
| `Anifilm` | [AniFilm](../trackers/anifilm.md) | `https://anifilm.pro` |
| `Leproduction` | [LE-Production](../trackers/leproduction.md) | `https://www.le-production.online` |
| `Viruseproject` | [ViruseProject](../trackers/viruseproject.md) | `https://viruseproject.tv` |
| `Bitru` | [BitRu](../trackers/bitru.md) | `https://bitru.org` |
| `Knaben` | [Knaben](../trackers/knaben.md) | `https://api.knaben.org` |
| `SubsPlease` | [SubsPlease](../trackers/subsplease.md) | `https://subsplease.org` |

Для всех трекеров `reqMinute` по умолчанию равен `8`, у `Megapeer` - `5`.

## Авторизация

Трекеры делятся на анонимные и требующие учётную запись. Для вторых есть два способа:

- **`cookie`** - войдите на трекер в браузере и скопируйте значение заголовка `Cookie` (например, `uid=…; pass=…`). Самый надёжный способ: повторный вход не нужен, пока cookie не истекла.
- **`login.u` / `login.p`** - CrabIndex сам выполнит вход и получит cookie. Поддерживается не всеми трекерами.

Какой способ поддерживает конкретный трекер и нужна ли ему авторизация вообще - смотрите на его странице в разделе [Трекеры](../trackers/overview.mdx). Некоторые трекеры (например, Anibelka) должны использоваться только анонимно: не задавайте для них `cookie` и `login`, иначе персональные passkey попадут в magnet-ссылки.

```yaml
# cookie
Kinozal:
  cookie: "uid=12345678; pass=XXXXXXXX"

# или логин и пароль
Toloka:
  login:
    u: "mylogin"
    p: "mypassword"
```

:::warning[Внимание]
Не храните настоящие логины, пароли и cookie в публичных репозиториях. Файл `init.yaml` должен быть доступен на чтение только пользователю сервиса (`chmod 600`).
:::

## Отключение трекеров

```yaml
disable_trackers:
  - megapeer
  - knaben
```

Для отключённых трекеров:

- раздачи не попадают в результаты поиска;
- трекер не отдаётся другим инстансам через `/sync/fdb/torrents`;
- трекер исчезает из `/api/v1.0/trackers` и списков Torznab;
- часть cron-задач сразу отвечает `disabled`.

Чтобы трекер гарантированно не парсился, уберите его задачи из crontab. Чтобы не принимать его при синхронизации, исключите его из `synctrackers`.

## Трекеры за Cloudflare

Если трекер отвечает страницей проверки Cloudflare («Just a moment…» или заголовок `cf-mitigated`), CrabIndex автоматически переключает этот хост на FlareSolverr и cffetch. Настройка - [FlareSolverr и cffetch](flaresolverr.md).

## Пример: Rutracker

```yaml
Rutracker:
  host: https://rutracker.org
  alias: ""               # Worker/onion, если FlareSolverr не используется
  reqMinute: 30
  topicFetchAttempts: 5
  log: true
```

Подробности по каждому трекеру - в разделе [Трекеры](../trackers/overview.mdx).
