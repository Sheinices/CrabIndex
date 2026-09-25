# Конфигурация

Config API читает и изменяет конфигурацию CrabIndex (`init.yaml` или `init.conf`) без доступа к файловой системе. Через него работает раздел **Настройки** [админ-панели](../admin.md). Устройство конфигурации и все ключи описаны в разделе [Обзор конфигурации](../configuration/overview.md).

## Доступ

Config API доступен **только через админ-панель**:

| Маршрут | Описание |
| --- | --- |
| `GET {admin.path}/api/config` | Текущий конфиг |
| `POST {admin.path}/api/config` | Сохранить конфиг |
| `GET {admin.path}/api/config/schema` | Схема полей |
| `POST {admin.path}/api/config/validate` | Проверить |
| `POST {admin.path}/api/config/diff` | Сравнить с действующим |
| `POST {admin.path}/api/config/parse` | Текст → JSON |
| `POST {admin.path}/api/config/render` | JSON → текст |
| `POST {admin.path}/api/config/format` | Нормализовать |

`{admin.path}` - путь админ-панели, по умолчанию `/admin`. Каждый запрос должен нести:

- cookie `crab_gate` - её ставит открытие адреса входа `{admin.path}?{admin.token}`;
- cookie `crab_session` - её ставит вход `POST {admin.path}/api/login` с телом `{"devkey": "…"}`;
- для `POST` - заголовок `X-Crab-Admin: 1`.

`apikey` и `X-Dev-Key` здесь не используются. Без cookie входа ответ такой же, как у несуществующего маршрута (`404`, или `401` при заданном `apikey`), без сессии - `401 {"error":"unauthorized"}`, `POST` без `X-Crab-Admin: 1` - `403`. Прямой запрос к внутренним маршрутам `/api/v1.0/config*` всегда получает `404`, из любой сети и с любым ключом.

Получить cookie из командной строки:

```bash
BASE=http://127.0.0.1:9117/admin
curl -s -c jar -b jar -o /dev/null "$BASE?ТОКЕН"
curl -s -c jar -b jar -H 'X-Crab-Admin: 1' -H 'Content-Type: application/json' \
  -d '{"devkey":"DEVKEY"}' "$BASE/api/login"
```

В примерах ниже `$BASE` и файл `jar` - из этого фрагмента. Подробнее о входе - [Админ-панель](../admin.md).

:::warning[Внимание]
`GET {admin.path}/api/config` возвращает конфиг **полностью**, включая `apikey`, `devkey`, `admin.token`, cookie и пароли трекеров, токен Alloha и пароли прокси. Список чувствительных полей приходит в `sensitiveFields`, чтобы клиент мог их скрыть.
:::

Ответы Config API сохраняют поля со значением `null`: редактор настроек на это рассчитывает.

## Тело POST-запросов

Все `POST`-маршруты принимают JSON-объект:

| Поле | Тип | Описание |
| --- | --- | --- |
| `data` | object | Конфиг в виде JSON-объекта. Если передан, `content` игнорируется |
| `content` | string | Конфиг текстом (YAML или JSON) |
| `format` | string | `yaml` или `json`. Для `content` задаёт формат разбора (без него формат определяется автоматически), для ответа задаёт формат выходного текста |

Конфиг всегда накладывается на значения по умолчанию: ключи объектов сливаются без учёта регистра, скаляры и массивы заменяются, `null` оставляет значение по умолчанию. Поэтому достаточно передать только изменяемые ключи. Но при **сохранении** в файл записывается полный нормализованный документ.

Если тело не удалось разобрать как JSON, ответ `500`. Ошибки содержимого возвращаются как `{"ok": false, "error": "..."}` с кодом `200`.

## GET config

Текущий конфиг, данные о файле и схема полей.

| Параметр | Тип | По умолчанию | Описание |
| --- | --- | --- | --- |
| `format` | string | формат файла конфига | Формат текста в `content`: `yaml` или `json` |

```bash
curl -b jar "$BASE/api/config?format=yaml"
```

```json
{
  "ok": true,
  "path": "init.yaml",
  "format": "yaml",
  "displayFormat": "yaml",
  "exists": true,
  "lastModifiedUtc": "2026-09-25T10:12:44.1234567Z",
  "data": { "listenip": "any", "listenport": 9117, "apikey": null, "...": "..." },
  "content": "---\nlistenip: any\nlistenport: 9117\n...",
  "schema": { "groups": [ "..." ] },
  "examplePath": "Data/example.yaml",
  "sensitiveFields": ["apikey", "devkey", "cookie", "u", "p", "username", "password", "token"],
  "note": "Полный конфиг. API доступен только из админ-панели."
}
```

| Поле | Описание |
| --- | --- |
| `path` | Файл конфига: `init.yaml` имеет приоритет над `init.conf`. Если файла нет, `null` |
| `format` | Формат файла: `yaml` или `json` (`init.conf` - JSON) |
| `displayFormat` | Формат, в котором отрисован `content` |
| `exists` | Существует ли файл конфига |
| `lastModifiedUtc` | Время изменения файла |
| `data` | Действующий конфиг целиком, с подставленными значениями по умолчанию |
| `content` | `data` текстом. В YAML-варианте поля `null` опускаются |
| `schema` | Схема полей для построения формы (то же, что `config/schema`) |
| `examplePath` | Путь к примеру конфига (`Data/example.yaml` или `Data/example.conf`) |
| `sensitiveFields` | Имена ключей, значения которых нужно скрывать |

## POST config

Проверяет конфиг и сохраняет его. Файл записывается атомарно и сразу перечитывается. Кроме того, CrabIndex раз в 10 секунд проверяет, не изменился ли файл, так что ручные правки тоже применяются без перезапуска.

Файл выбирается так: если конфиг уже существует, перезаписывается он. Иначе создаётся `init.yaml` (или `init.conf` при `format: json`). Формат вывода - `format` из запроса, иначе формат существующего файла, иначе YAML.

```bash
# Изменить пару ключей
curl -b jar -X POST "$BASE/api/config" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"data": {"openstats": false, "Rutor": {"reqMinute": 6}}}'

# Сохранить YAML-текст
curl -b jar -X POST "$BASE/api/config" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"content": "listenport: 9117\nopenstats: false\n", "format": "yaml"}'
```

```json
{
  "ok": true,
  "path": "init.yaml",
  "format": "yaml",
  "lastModifiedUtc": "2026-09-25T10:15:02.0000000Z",
  "message": "Конфигурация сохранена. Изменения применятся автоматически."
}
```

Если меняются `admin.path`, `admin.token` или `devkey`, новые значения действуют сразу после сохранения: старый адрес входа перестаёт работать, а смена `devkey` завершает все сессии, включая текущую. См. [Админ-панель](../admin.md).

Если валидация не прошла, файл не меняется, а ответ содержит первую ошибку:

```json
{ "ok": false, "error": "tracksmod: допустимы только 0 или 1" }
```

:::note[Примечание]
`POST` без `data` и `content` возвращает `{"ok": false, "error": "Укажите data или content"}`, пустое тело - `Тело запроса пусто`.
:::

## POST config/validate

Проверяет конфиг без сохранения.

```bash
curl -b jar -X POST "$BASE/api/config/validate" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"data": {"listenport": 70000, "disable_trackers": ["foo"]}}'
```

```json
{
  "ok": false,
  "error": "listenport: значение должно быть от 1 до 65535",
  "errors": ["listenport: значение должно быть от 1 до 65535"],
  "warnings": ["disable_trackers: неизвестный трекер «foo»"]
}
```

`errors` мешают сохранению, `warnings` нет. Примеры проверок:

- `listenport` 1..65535;
- `tracksmod` только 0 или 1;
- `timeSync`, `timeStatsUpdate`, `maxreadfile`, `tracksatempt` и другие интервалы ≥ 1;
- `search.mergeV1` только `auto`, `true` или `false`;
- `logging.cronSkipFastMs` ≥ 0;
- `admin.path` - один сегмент `[a-z0-9_-]` длиной 2-32, не служебный путь;
- `admin.token` - только `A-Z`, `a-z`, `0-9`, длина 12-64;
- `admin.sessionHours` 1..8760.

Предупреждения выдаются для неизвестных slug в `synctrackers` и `disable_trackers`, некорректных URL в `tsuri`, `listenip`, который не равен `any` и не является IP-адресом, `fdbPathLevels` вне диапазона 1-4 и пустого `admin.token` (панель будет недоступна до перезапуска, при котором токен сгенерируется).

## POST config/diff

Сравнивает предложенный конфиг с действующим. Изменения вычисляются после наложения на значения по умолчанию, поэтому ключи, совпадающие с текущими, в список не попадают.

```bash
curl -b jar -X POST "$BASE/api/config/diff" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"data": {"openstats": false, "Rutor": {"reqMinute": 6}}}'
```

```json
{
  "ok": true,
  "diffs": [
    { "path": "openstats", "oldValue": "true", "newValue": "false", "sensitive": false, "change": "changed" },
    { "path": "Rutor.reqMinute", "oldValue": "8", "newValue": "6", "sensitive": false, "change": "changed" }
  ],
  "changeCount": 2,
  "validation": { "ok": true, "error": null, "errors": [], "warnings": [] }
}
```

| Поле | Описание |
| --- | --- |
| `path` | Путь к ключу через точку |
| `oldValue` / `newValue` | Значения в виде строк (`null`, если ключа нет) |
| `sensitive` | Ключ относится к чувствительным (`apikey`, `cookie`, `login.p` и т. п.). Значения при этом **не** маскируются |
| `change` | `added`, `removed` или `changed` |

## POST config/parse

Разбирает текст в нормализованный JSON-документ (полный конфиг со значениями по умолчанию). Редактор использует его для переключения между текстовым режимом и формой.

```bash
curl -b jar -X POST "$BASE/api/config/parse" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"content": "listenport: 9200", "format": "yaml"}'
```

```json
{ "ok": true, "data": { "listenip": "any", "listenport": 9200, "...": "..." } }
```

Без `content` ответ `{"ok": false, "error": "Укажите content"}`. Ошибка синтаксиса YAML или JSON возвращается в `error`.

## POST config/render

Превращает `data` в текст без нормализации и проверки.

```bash
curl -b jar -X POST "$BASE/api/config/render" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"data": {"listenport": 9117, "apikey": null}, "format": "yaml"}'
```

```json
{ "ok": true, "content": "---\nlistenport: 9117\n", "format": "yaml" }
```

В YAML поля `null` опускаются, в JSON сохраняются. Без `data` ответ `{"ok": false, "error": "Укажите data"}`.

## POST config/format

Разбирает, проверяет и нормализует конфиг и возвращает полный документ в виде объекта и текста. Удобно, чтобы привести рукописный конфиг к каноническому виду.

```bash
curl -b jar -X POST "$BASE/api/config/format" \
  -H "X-Crab-Admin: 1" -H "Content-Type: application/json" \
  -d '{"content": "{\"listenport\": 9117}", "format": "yaml"}'
```

Ответ: `{ "ok": true, "data": {...}, "content": "---\n...", "format": "yaml" }`. Если проверка не прошла, возвращается `{ "ok": false, "error": "..." }`.

:::note[Примечание]
Здесь `format` определяет формат **вывода**. Формат входного `content` при этом берётся из того же поля, поэтому JSON-текст с `format: yaml` тоже разберётся: YAML является надмножеством JSON.
:::

## GET config/schema

Схема полей, по которой раздел **Настройки** админ-панели строит форму.

```bash
curl -b jar "$BASE/api/config/schema"
```

```json
{
  "ok": true,
  "schema": {
    "groups": [
      {
        "id": "server",
        "title": "Сервер",
        "description": "Прослушивание и ключи доступа",
        "fields": [
          { "key": "listenport", "type": "int", "label": "Порт", "description": "1-65535", "sensitive": false, "min": 1, "max": 65535, "enumValues": null }
        ]
      }
    ]
  }
}
```

Каждое поле описывается ключом, типом (`string`, `int`, `bool`, `password`, `select`, `stringList`, `json`), подписью, признаком `sensitive`, ограничениями `min`/`max` и списком допустимых значений `enumValues`. У группы трекеров вместо `fields` есть массив `trackers` с полями `alias`, `useproxy`, `reqMinute`, `log`, `cookie`, `login.u`, `login.p` для каждого трекера.

## См. также

- [Обзор конфигурации](../configuration/overview.md)
- [Админ-панель](../admin.md)
- [Матрица доступа](../operations/access-matrix.md)
