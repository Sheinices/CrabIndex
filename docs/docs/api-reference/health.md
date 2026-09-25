# Health

Диагностические эндпоинты: живость процесса, версия сборки, время последнего обновления базы и список фоновых задач. Все они публичные: не требуют ни `apikey`, ни `devkey` и принимают любой HTTP-метод. Их удобно использовать в healthcheck Docker, пробах Kubernetes и системах мониторинга.

## GET /health

Проверка живости. Если процесс отвечает, возвращается `200`.

```bash
curl http://127.0.0.1:9117/health
```

```json
{ "status": "OK" }
```

Образ Docker уже содержит такую проверку:

```dockerfile
HEALTHCHECK --interval=30s --timeout=15s --start-period=45s --retries=3 --start-interval=5s \
    CMD curl -f -s --max-time 10 http://127.0.0.1:9117/health || exit 1
```

Для своего `docker-compose.yml`:

```yaml
services:
  crabindex:
    image: crabindex:latest
    healthcheck:
      test: ["CMD", "curl", "-f", "-s", "--max-time", "10", "http://127.0.0.1:9117/health"]
      interval: 30s
      timeout: 15s
      retries: 3
```

## GET /version

Версия, git-коммит, ветка и дата сборки. Значения зашиваются при компиляции. Версия берётся из git-тега (`v0.1.0` → `0.1.0`). Если сборка сделана не с тега, версия получается вида `<последний тег>-next+<sha>`, а в репозитории без тегов - `<версия из Cargo.toml>-dev+<sha>` (например, `0.1.0-dev+c704d032`). Каждое поле можно переопределить переменными окружения сборки `CRABINDEX_VERSION`, `CRABINDEX_GIT_SHA`, `CRABINDEX_GIT_BRANCH`, `CRABINDEX_BUILD_DATE`.

```bash
curl http://127.0.0.1:9117/version
```

```json
{
  "version": "0.1.0",
  "gitSha": "c704d032",
  "gitBranch": "main",
  "buildDate": "2026-09-20 12:00:00 UTC"
}
```

## GET /lastupdatedb

Время самого свежего `updateTime` среди всех бакетов FileDB (`masterDb`) в формате `дд.ММ.гггг ЧЧ:мм`. Пустая база даёт заглушку `01.01.2000 01:01`.

```bash
curl http://127.0.0.1:9117/lastupdatedb
```

```json
{ "lastupdatedb": "25.09.2026 14:05" }
```

## GET /health/background-jobs

Фоновые задачи, которые выполняются прямо сейчас: `ParseAllTask` и `UpdateTasksParse` трекеров, а также проверка FileDB (`/cron/maintenance/Check`). Этот же список показывают разделы **Обзор** и **Задачи** [админ-панели](../admin.md).

Список хранится в памяти процесса и сразу после перезапуска пуст. Незавершённые циклы на диске показывает [`/cron/maintenance/ParseAllStatus`](cron.md#get-cronmaintenanceparseallstatus).

```bash
curl http://127.0.0.1:9117/health/background-jobs
```

```json
{
  "jobs": [
    {
      "id": "anibelka:parsealltask",
      "tracker": "anibelka",
      "job": "ParseAllTask",
      "startedAtUtc": "2026-09-08T13:48:01.5624351Z",
      "elapsedSeconds": 940,
      "pagesCompleted": 6,
      "pagesTotal": 154,
      "percent": 4,
      "currentCategory": "32",
      "currentPage": 5,
      "summary": "6/154 pages · category 32 · page 5"
    }
  ]
}
```

### Поля задачи

| Поле | Тип | Описание |
| --- | --- | --- |
| `id` | string | Ключ задачи `{tracker}:{job}` в нижнем регистре |
| `tracker` | string | Slug трекера (для проверки FileDB `maintenance`) |
| `job` | string | `ParseAllTask`, `UpdateTasksParse` или `Check` |
| `startedAtUtc` | string | Время старта, UTC, ISO 8601 с 7 знаками дробной части |
| `elapsedSeconds` | integer | Сколько секунд задача уже идёт |
| `pagesCompleted` | integer | Сколько страниц листинга пройдено в текущем цикле. Это попытки, а не число новых раздач |
| `pagesTotal` | integer | Сколько страниц было в очереди на старте. У `UpdateTasksParse` обычно `0` |
| `percent` | integer | `pagesCompleted / pagesTotal`, округлено, не больше 100. Если `pagesTotal` = 0, поля нет |
| `currentCategory` | string | Раздел, который обходится сейчас (`32`, `serial-hd` и т. п.). Если раздела нет, поля нет |
| `currentPage` | integer | Номер страницы внутри `currentCategory`. Если страницы нет, поля нет |
| `summary` | string | Строка для консоли: `6/154 pages · category 32 · page 5`, или `running`, если прогресс неизвестен |

:::note[Примечание]
Синхронные запуски `parse` и `ParseLatest` в этот список не попадают. Парсеры, у которых есть только `parse` (anistar, leproduction, viruseproject, anifilm и др.), тоже здесь не видны. Следите за ними по `Data/log/{tracker}.log` и строкам `cron:` в консоли.
:::

## GET /api/v1.0/conf

Публичная проверка ключа и маркер совместимости. Описан на отдельной странице: [Идентификация](conf.md).

## См. также

- [Cron](cron.md): запуск задач и статус циклов ParseAll
- [Фоновые процессы](../concepts/background-jobs.md)
- [Решение проблем](../operations/troubleshooting.md)
