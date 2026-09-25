# Добавление трекера

Трекер считается добавленным только после полной проводки: код парсера, маршруты `/cron/…`, блок конфигурации, схема настроек админ-панели, действия в админ-панели, списки трекеров, crontab, OpenAPI, иконка, тесты и страница документации. Не оставляйте «полуподключённый» трекер.

Ориентируйтесь на ближайший существующий трекер той же формы, а не придумывайте новый цикл задач. Ниже структура разобрана на примере Rutor (`crates/crab-trackers-a/src/rutor`).

## Формы трекеров

| Форма | Пример | Задачи cron |
| --- | --- | --- |
| Полный обход (parse + UpdateTasksParse + ParseAllTask [+ ParseLatest]) | rutor, kinozal, korsars, anibelka, ultradox, rutracker | Частый `parse`, ночной `UpdateTasksParse`, `ParseAllTask` раз-два в сутки |
| Диапазон страниц (`limit_page` / `parseFrom`-`parseTo`) | anidub, anistar, leproduction, rudub, baibako | Только `parse` с параметрами |
| API: свежие + архив (`backfill`) | bitru, knaben, subsplease | `parse` + `backfill` / `ParseShows` с сохранением позиции |
| Особые | lostfilm, mazepa | Собственная логика обхода |

## Где живёт код

Трекеры сгруппированы в крейты `crab-trackers-a` … `crab-trackers-e`. Новый трекер добавляется модулем в один из них (обычно в тот, где уже есть похожие). У каждого крейта:

```rust
// crates/crab-trackers-a/src/lib.rs
pub mod common;     // общие помощники крейта: flat_parse_all, load_task_map, q_i32, …
pub mod rutor;

/// Регистрация при старте сервера.
pub fn init() {
    use crab_core::trackers::register_parse_all_starter;
    register_parse_all_starter(Arc::new(rutor::Starter));          // для ResumeParseAll
    crab_core::fdb::register_id_extractor(kinozal::TRACKER_NAME, kinozal::url_id); // если нужно
}

/// Маршруты крейта (пути в нижнем регистре).
pub fn router() -> axum::Router {
    axum::Router::new().merge(rutor::router())
}
```

Бинарник `crabindex` вызывает `init()` и `router()` каждого крейта (`crates/crabindex/src/main.rs`, `app.rs`). Если вы добавляете трекер в существующий крейт, трогать сервер не нужно.

Модуль трекера обычно состоит из трёх файлов:

| Файл | Содержимое |
| --- | --- |
| `mod.rs` | `TRACKER_NAME`, блокировки и флаги задач, загрузка страниц, функции `parse` / `update_tasks_parse` / `parse_all_task` / `parse_latest`, `Starter`, `router()` |
| `parser.rs` | Чистые функции разбора HTML/JSON в `TorrentDetails`, определение последней страницы, проверка «это настоящий листинг» |
| `categories.rs` | Карта категорий трекера → типы FileDB (`movie`, `serial`, `anime`, …) и порядок обхода |

## API crab-core

### Сеть: `crab_core::net`

```rust
use crab_core::net::{self, Req};

let html = net::get(&url, &Req::new()
    .useproxy(conf().Rutor.useproxy)   // пул proxy при useproxy: true
    .cookie_opt(cookie)                // сессия трекера
    .cp1251()                          // если сайт в windows-1251
    .referer(&referer)
    .cancel(&ct)).await;               // Option<String>; None при любой ошибке
```

Также есть `net::get_json`, `net::post`, `net::post_json`, `net::download` (для `.torrent`) и `net::raw_client` для нестандартных сценариев входа. Прокси (`globalproxy`, `proxy`) и обход Cloudflare (FlareSolverr/cffetch при `cf-mitigated` или «Just a moment…») применяются автоматически. Для запросов используйте `conf().{Трекер}.rq_host()` (учитывает `alias`), а в URL раздач, которые попадут в FileDB, - `host`.

### Регулярные выражения: `crab_core::rx`

Кешируемые регулярки на `fancy_regex` (поддерживают lookaround и обратные ссылки). Функции `group`, `groups`, `all_groups`, `captures`, `is_match`, `replace`, `split` и их варианты `_i` (без учёта регистра). Отсутствующая группа возвращается как пустая строка.

### Запись в FileDB: `crab_core::fdb`

```rust
let mut t = TorrentDetails::new(TRACKER_NAME, meta.types, url, title);
t.sid = …; t.pir = …; t.sizeName = …; t.magnet = …;
t.createTime = …; t.name = …; t.originalname = …; t.relased = year;

fdb::add_or_update(&torrents);   // группирует по бакетам и обновляет
```

Если magnet нужно получить отдельным запросом (страница раздачи, `.torrent`), используйте асинхронный вариант: шаг получает новую запись и уже сохранённую с тем же URL и решает, скачивать ли заново.

```rust
fdb::add_or_update_async(torrents, fdb::by_url, |mut t, cached| async move {
    if let Some(c) = cached {
        if c.title == t.title && !c.magnet.is_empty() {
            t.magnet = c.magnet;          // ничего не изменилось - не качаем
            return Some(t);
        }
    }
    let bytes = net::download(&torrent_url, &Req::new()).await?;
    t.magnet = crab_core::parsing::bencode::magnet(&bytes)?;
    Some(t)                               // None - пропустить запись
}).await;
```

Для трекеров с персональным passkey в announce используйте `bencode::magnet_no_trackers`, чтобы passkey не попал в magnet. Поля поиска, размер, качество и озвучки FileDB вычисляет сама при записи.

Если URL раздачи может меняться (slug в адресе), зарегистрируйте `fdb::register_id_extractor(TRACKER_NAME, fn(&str) -> i32)`, чтобы запись обновлялась, а не дублировалась.

### Задачи: `crab_core::trackers`

| Функция | Для чего |
| --- | --- |
| `run_parse(tracker, &PARSE_LOCK, check_disabled, action)` | Ежечасный `parse`: не больше одного на трекер, иначе ответ `work` |
| `run_update_tasks_parse_in_background(tracker, &FLAG, check_disabled, action)` | `UpdateTasksParse` в фоне, сразу `ok` / `work`, лимит 30 минут |
| `run_parse_all_task_in_background(tracker, &FLAG, check_disabled, action)` | `ParseAllTask` в фоне, отмена после 45 минут без прогресса |
| `run_parse_latest(tracker, &LATEST_LOCK, check_disabled, build_log)` | `ParseLatest`, не пересекается с ParseAll и UpdateTasks |
| `check(&ct)?`, `sleep(ms, &ct)` | Проверка отмены (остановка сервера, watchdog) |
| `yield_to_hourly_parse_and_throttle(...)` | Пауза между страницами обхода: уступить `parse` и выдержать `reqMinute` |
| `report_progress(...)` | Прогресс для `/health/background-jobs` и раздела **Задачи** админ-панели |

При `check_disabled = true` задача сразу отвечает `disabled` для трекеров из `disable_trackers`.

Цикл ParseAll хранится в `crab_core::trackers::cycle`: карта страниц `Data/temp/{трекер}_taskParse.json` и состояние цикла `Data/temp/{трекер}_parseAllCycle.json` (`begin_flat_full_run`, `note_attempt`, `persist_after_page_if_needed`, …). Для плоской карты «категория → страницы» готовые циклы уже собраны в `common::flat_parse_all` и `common::flat_parse_latest` крейта:

```rust
pub fn parse_all_task() -> String {
    trackers::run_parse_all_task_in_background(TRACKER_NAME, &PARSE_ALL_TASK_WORK, false, |ct| async move {
        common::flat_parse_all(TRACKER_NAME, &TASK_PARSE, TASK_PARSE_PATH, &PARSE_LOCK,
            || conf().Rutor.parse_delay(), ct, parse_page).await
    })
}
```

Чтобы незавершённый цикл продолжался после рестарта, реализуйте `trackers::ParseAllStarter` и зарегистрируйте его в `init()`.

### Журнал: `crab_core::parsing::parser_log`

```rust
parser_log::write(TRACKER_NAME, format!("Category {cat}: page {page}"));
parser_log::write_stats(TRACKER_NAME, "done", parsed, processed, updated, failed);
```

Пишет в `Data/log/{трекер}.log`, если включены `logParsers` и `log` в блоке трекера.

## Маршруты

```rust
pub fn router() -> Router {
    Router::new()
        .route("/cron/rutor/parse", any(|Query(q): Query<HashMap<String, String>>| async move {
            parse(q_i32(&q, "page", 0)).await
        }))
        .route("/cron/rutor/updatetasksparse", any(|| async { update_tasks_parse() }))
        .route("/cron/rutor/parsealltask", any(|| async { parse_all_task() }))
        .route("/cron/rutor/parselatest", any(|Query(q): Query<HashMap<String, String>>| async move {
            parse_latest(q_i32(&q, "pages", 5)).await
        }))
}
```

Пути регистрируются **в нижнем регистре**: сервер приводит к нему путь и имена параметров до маршрутизации, поэтому `/cron/Rutor/ParseAllTask?Page=2` попадёт сюда же. Всё под `/cron/` автоматически получает политику DevAdmin.

## Чек-лист проводки

1. **Код** - модуль в `crates/crab-trackers-*/src/{трекер}/`, `pub mod` и регистрация в `lib.rs` (`init()`, `router()`).
2. **Конфигурация** - в `crates/crab-core/src/config.rs`: поле в `AppOptions`, значение по умолчанию (`TrackerSettings::new(host, reqMinute)`), `trackers_mut()`, `tracker()` и `TRACKER_SLUGS`.
3. **Схема настроек** - списки трекеров в `crates/crabindex/src/config_api/schema.rs` (по ним строится форма в разделе **Настройки** админ-панели).
4. **Списки трекеров** - `KNOWN_TRACKER_SLUGS` в `crates/crab-search/src/search/tracker_catalog.rs`.
5. **Шаблоны** - блок трекера (без секретов) в `Data/example.yaml` и `Data/example.conf`, slug в `synctrackers`.
6. **Расписание** - задачи в `Data/crontab` через `run-job.sh`, с минутами, не совпадающими с соседями.
7. **OpenAPI** - slug и маршруты в `web/public/openapi.yaml`.
8. **Интерфейсы** - подпись трекера в `web/src/lib/torrents.js`; доступные cron-действия и их параметры в `admin/src/lib/actions.js` (`TRACKER_ACTIONS`, при необходимости `PAGE_PARSE` и `PARAM_OVERRIDES`) - по ним раздел **Трекеры** админ-панели строит кнопки запуска.
9. **Иконка** - `web/public/img/ico/{трекер}.ico` и `docs/static/img/trackers/{трекер}.ico` (с сайта самого трекера).
10. **Тесты** - сохранённые страницы в `tests/fixtures/{трекер}/` и тесты `tests/{трекер}.rs`; `cargo test -p crab-trackers-*`.
11. **Документация** - страница `docs/docs/trackers/{трекер}.md`, строка в `docs/sidebars.js` и `docs/src/data/trackers.js` и в [обзоре трекеров](../trackers/overview.mdx), упоминания в [Конфигурации трекеров](../configuration/trackers.md), [Cron](../deployment/cron.md) и [Cron API](../api-reference/cron.md).

## Правила

- Последнюю страницу берите из **пагинатора этого листинга**, а не из глобальных счётчиков или чисел в `<script>` - иначе карта страниц раздувается.
- `UpdateTasksParse` должен **удалять** страницы за концом пагинатора (`prune_pages_beyond_max`), но пустой ответ не должен обнулять карту: сбой загрузки ≠ «ноль страниц».
- Проверяйте, что ответ - настоящий листинг (как `looks_like_browse_listing` у Rutor), иначе страница проверки Cloudflare или ошибка будут засчитаны как пустая страница.
- Трекер-преемник закрытого сайта получает **новый** slug; не переназначайте старый.
- Не коммитьте рабочие cookie и пароли, даже в тестовые фикстуры.
- Тесты парсера должны работать офлайн, на фикстурах.
