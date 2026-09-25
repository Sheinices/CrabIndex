# Alloha

CrabIndex не хранит соответствие «ID фильма → название». Когда клиент ищет по идентификатору внешней базы, сервер спрашивает названия у **Alloha TV API** (v2), а затем ищет их в FileDB.

Поддерживаемые запросы:

| Формат | Пример |
| --- | --- |
| IMDb | `tt0133093` |
| Кинопоиск | `kp301` |
| TMDB | `tmdb603` или `tmdb:603` |
| Ссылка TMDB | `https://www.themoviedb.org/movie/603-the-matrix`, `https://www.themoviedb.org/tv/1396` |

## Настройка

```yaml
alloha:
  enable: true
  baseUrl: https://apbugall.org
  token: "ВАШ_ТОКЕН"
  timeoutSeconds: 10
  cacheHours: 24
  filterByYear: true
```

| Параметр | По умолчанию | Описание |
| --- | --- | --- |
| `enable` | `true` | Разрешать ID через Alloha |
| `baseUrl` | `https://apbugall.org` | Адрес Alloha API |
| `token` | встроенный токен | Bearer-токен Alloha |
| `timeoutSeconds` | `8` (в шаблоне `10`) | Таймаут запроса к Alloha, секунды |
| `cacheHours` | `24` | Сколько часов хранить результат в памяти. `0` или меньше - используется 24 |
| `filterByYear` | `true` | Если клиент не передал год, оставлять раздачи с годом из Alloha ±1 (и раздачи без года) |

:::note[Примечание]
И в коде, и в `Data/example.yaml` уже есть рабочий токен по умолчанию. Если у вас есть свой токен Alloha, укажите его.
:::

## Как это работает

1. Клиент отправляет, например, `GET /api/v2.0/indexers/all/results?query=tt0133093`.
2. CrabIndex распознаёт идентификатор и запрашивает `{baseUrl}/v2/movies/search?imdb=tt0133093` (для Кинопоиска - `kp=…`, для TMDB - `tmdb=…`) с заголовком `Authorization: Bearer {token}`.
3. Из ответа берутся оригинальное название, русское название, альтернативное название, год и тип (фильм / сериал / аниме).
4. Выполняется точный поиск по этим названиям в FileDB; результаты фильтруются по году (`filterByYear`) и типу.

Результат кешируется в памяти под всеми известными идентификаторами фильма (IMDb, Кинопоиск, TMDB) на `cacheHours` часов. После перезапуска кеш пуст.

Если Alloha выключена, токен пуст, запрос не удался или фильм не найден, строка запроса ищется как обычный текст - пустого ответа из-за Alloha не будет, но и совпадений по ID, скорее всего, тоже.

## Проверка

```bash
# Поиск по IMDb через CrabIndex
curl "http://localhost:9117/api/v1.0/torrents?search=tt0133093&apikey=KEY"

# Прямой запрос к Alloha
curl -H "Authorization: Bearer $TOKEN" "https://apbugall.org/v2/movies/search?imdb=tt0133093"
```

:::warning[Внимание]
Клиенты вроде Prisma и Lampa часто ищут по ID. Без рабочей Alloha такие запросы вернут мало результатов или не вернут ничего.
:::
