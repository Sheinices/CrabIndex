# Синхронизация

Sync API отдаёт содержимое FileDB другим экземплярам по частям, с учётом изменений. По этому протоколу работает публичный сервер синхронизации `https://sync.crab.rip`, и точно так же один экземпляр CrabIndex может реплицировать базу с другого. Клиентская часть (фоновый worker, ключ `syncapi`) описана в разделе [Синхронизация](../concepts/sync.md).

## Доступ

Маршруты `/sync/*` публичные: ни `apikey`, ни `devkey` не проверяются. Отдачу данных включает флаг `opensync`:

```yaml
opensync: true # по умолчанию true; false - /sync/fdb и /sync/fdb/torrents отдают пустые ответы
```

Раздачи трекеров из `disable_trackers` в выгрузку не попадают.

## GET /sync/conf

Возможности протокола. Отвечает всегда, независимо от `opensync`.

```bash
curl http://127.0.0.1:9117/sync/conf
```

```json
{ "fbd": true, "spidr": true, "version": 2 }
```

| Поле | Описание |
| --- | --- |
| `fbd` | Сервер поддерживает выгрузку по бакетам (`/sync/fdb/torrents`). Имя поля историческое и сохранено ради совместимости протокола |
| `spidr` | Поддерживается облегчённый режим `spidr=true` |
| `version` | Версия протокола, `2` |

Клиент CrabIndex проверяет `fbd` перед синхронизацией. Если флага нет, в журнал пишется предупреждение о том, что хост `syncapi` нужно обновить.

## GET /sync/fdb/torrents

Основной эндпоинт. Отдаёт бакеты FileDB, изменённые после метки `time`, в порядке возрастания `fileTime`. Бакеты накапливаются в ответе, пока суммарное число раздач не превысит 2000, поэтому одна страница может немного превышать это число.

### Параметры

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `time` | integer | `0` | Метка `fileTime`: вернуть бакеты с `fileTime > time`. Для полной выгрузки передайте `-1`. При `0` ответ пустой |
| `start` | integer | `-1` | Метка начала предыдущей синхронизации. Раздачи с `updateTime` старше неё отдаются в сокращённом виде (`sid`, `pir`, `url`) |
| `spidr` | boolean | `false` | Облегчённый режим: все раздачи только с `sid`, `pir`, `url`. Используется для быстрого обновления сидов и пиров |

`fileTime` - метка времени в формате Windows FILETIME: число 100-наносекундных интервалов с 1601-01-01 UTC.

```bash
# Первая страница полной выгрузки
curl "http://127.0.0.1:9117/sync/fdb/torrents?time=-1"

# Следующая страница: time = fileTime последней коллекции предыдущего ответа
curl "http://127.0.0.1:9117/sync/fdb/torrents?time=134035812000000000"

# Только сиды/пиры
curl "http://127.0.0.1:9117/sync/fdb/torrents?time=-1&spidr=true"
```

### Ответ

```json
{
  "nextread": true,
  "countread": 2014,
  "take": 2000,
  "collections": [
    {
      "Key": "матрица:the matrix",
      "Value": {
        "time": "2026-09-25T11:58:40.1234567Z",
        "fileTime": 134035811201234567,
        "torrents": {
          "https://rutracker.org/forum/viewtopic.php?t=123456": {
            "trackerName": "rutracker",
            "types": ["movie"],
            "url": "https://rutracker.org/forum/viewtopic.php?t=123456",
            "title": "Матрица / The Matrix (1999) BDRip 1080p",
            "sid": 42,
            "pir": 3,
            "sizeName": "12.4 GB",
            "createTime": "2020-01-10T00:00:00Z",
            "updateTime": "2026-09-25T11:58:40.1234567Z",
            "checkTime": "2026-09-25T11:58:40.1234567Z",
            "magnet": "magnet:?xt=urn:btih:...",
            "name": "матрица",
            "originalname": "the matrix",
            "relased": 1999
          }
        }
      }
    }
  ]
}
```

| Поле | Описание |
| --- | --- |
| `nextread` | `true`, если есть ещё данные. Запросите следующую страницу с `time` = `fileTime` последней коллекции |
| `countread` | Сколько раздач в этой странице |
| `take` | Порог страницы, всегда `2000` |
| `collections[].Key` | Ключ бакета FileDB (`name:originalname`) |
| `collections[].Value.time` | `updateTime` бакета |
| `collections[].Value.fileTime` | `fileTime` бакета, служит курсором |
| `collections[].Value.torrents` | Словарь «URL раздачи → раздача». Если для раздачи уже известны дорожки, заполняются `ffprobe` и `languages` |

При `opensync: false` или `time=0` ответ такой: `{"nextread": false, "collections": []}`.

### Алгоритм клиента

1. Запросите `time=-1` (или сохранённую метку).
2. Импортируйте `collections`, запомните `fileTime` последней коллекции.
3. Пока `nextread: true`, повторяйте запрос с новым `time`.
4. В следующий раз начните с сохранённой метки.

Встроенный клиент CrabIndex хранит курсоры в `Data/temp/lastsync.txt` и `Data/temp/starsync.txt`. Полный цикл повторяется каждые `timeSync` минут, spidr-проход каждые `timeSyncSpidr` минут (оба не чаще раза в 20 минут). Фильтры `synctrackers` и `syncsport` применяются на стороне клиента, см. [Синхронизация](../concepts/sync.md).

:::note[Примечание]
Упорядоченный снимок `masterDb` кешируется и перестраивается раз в 10 минут. Свежие изменения могут появиться в выдаче с такой задержкой.
:::

## GET /sync/fdb

Отладочный поиск бакетов по подстроке ключа. Возвращает не больше 20 совпадений вместе с полным содержимым.

| Параметр | Тип | Описание |
| --- | --- | --- |
| `key` | string | Подстрока ключа бакета (обязателен; без него ответ `500`) |

```bash
curl "http://127.0.0.1:9117/sync/fdb?key=matrix"
```

```json
[
  {
    "Key": "матрица:the matrix",
    "updateTime": "2026-09-25T11:58:40.1234567Z",
    "fileTime": 134035811201234567,
    "path": "Data/fdb/3f/5a0c...",
    "value": {
      "https://rutracker.org/forum/viewtopic.php?t=123456": { "trackerName": "rutracker", "title": "..." }
    }
  }
]
```

При `opensync: false` возвращается `[]`.

## GET /sync/torrents

Маршрут старой версии протокола. Всегда возвращает подсказку:

```json
{ "error": "use GET /sync/fdb/torrents" }
```

## См. также

- [Синхронизация (концепция)](../concepts/sync.md)
- [FileDB](../concepts/filedb.md)
- [Матрица доступа](../operations/access-matrix.md)
