//! Settings form schema (groups/fields for the admin settings UI) and model validation rules.

use crab_core::config::AppOptions;
use serde_json::{json, Value};

pub const KNOWN_TRACKER_SLUGS: [&str; 25] = [
    "rutracker", "rutor", "kinozal", "nnmclub", "megapeer", "bitru", "toloka", "mazepa", "lostfilm", "baibako", "torrentby",
    "selezen", "animelayer", "anidub", "anistar", "anibelka", "aniliberty", "knaben", "leproduction", "viruseproject",
    "anifilm", "korsars", "ultradox", "rudub", "subsplease",
];

pub const TRACKER_BLOCK_NAMES: [&str; 25] = [
    "Rutor", "Megapeer", "TorrentBy", "Kinozal", "NNMClub", "Bitru", "Toloka", "Mazepa", "Rutracker", "Selezen", "Lostfilm",
    "Animelayer", "Anidub", "Anistar", "Anibelka", "Aniliberty", "Knaben", "Baibako", "Leproduction", "Viruseproject",
    "Anifilm", "Korsars", "Ultradox", "Rudub", "SubsPlease",
];

/// Field names whose values are secrets (case-insensitive).
pub const SENSITIVE_FIELD_NAMES: [&str; 8] = ["apikey", "devkey", "cookie", "u", "p", "username", "password", "token"];

pub fn is_sensitive_field(name: &str) -> bool {
    SENSITIVE_FIELD_NAMES.iter().any(|s| s.eq_ignore_ascii_case(name))
}

pub fn is_known_tracker(slug: &str) -> bool {
    KNOWN_TRACKER_SLUGS.iter().any(|s| s.eq_ignore_ascii_case(slug))
}

fn sorted_slugs() -> Vec<&'static str> {
    let mut v = KNOWN_TRACKER_SLUGS.to_vec();
    v.sort();
    v
}

#[derive(Default)]
struct F {
    sensitive: bool,
    min: Option<i32>,
    max: Option<i32>,
    enum_values: Option<Vec<String>>,
}

fn field(key: &str, ty: &str, label: &str, description: Option<&str>, o: F) -> Value {
    json!({
        "key": key,
        "type": ty,
        "label": label,
        "description": description,
        "sensitive": o.sensitive,
        "min": o.min,
        "max": o.max,
        "enumValues": o.enum_values,
    })
}

fn fd(key: &str, ty: &str, label: &str, description: Option<&str>) -> Value {
    field(key, ty, label, description, F::default())
}

fn min(v: i32) -> F {
    F { min: Some(v), ..F::default() }
}

fn secret() -> F {
    F { sensitive: true, ..F::default() }
}

fn enums(v: &[&str]) -> F {
    F { enum_values: Some(v.iter().map(|s| s.to_string()).collect()), ..F::default() }
}

fn group(id: &str, title: &str, description: Option<&str>, fields: Vec<Value>) -> Value {
    json!({ "id": id, "title": title, "description": description, "fields": fields })
}

fn tracker_groups() -> Value {
    let mut names = TRACKER_BLOCK_NAMES.to_vec();
    names.sort_by_key(|n| n.to_lowercase());
    let trackers: Vec<Value> = names
        .into_iter()
        .map(|name| {
            json!({
                "id": name,
                "title": name,
                "description": Value::Null,
                "trackerSlug": name.to_lowercase(),
                "fields": [
                    fd("alias", "string", "Alias URL", Some("Onion/worker URL")),
                    fd("useproxy", "bool", "Use proxy", None),
                    field("reqMinute", "int", "Запросов/мин", Some("-1 - отключить"), min(-1)),
                    field("topicFetchAttempts", "int", "Попыток topic GET", Some("Rutracker: ретраи magnet/details за прогон (дефолт 5)"), min(1)),
                    fd("log", "bool", "Лог парсера", Some("Data/log/{tracker}.log, default: true")),
                    field("cookie", "password", "Cookie", Some("Статический cookie"), secret()),
                    field("login.u", "password", "Login", None, secret()),
                    field("login.p", "password", "Password", None, secret()),
                ]
            })
        })
        .collect();
    json!({
        "id": "trackers",
        "title": "Трекеры",
        "description": "Настройки парсеров (host задаётся по умолчанию)",
        "trackers": trackers,
    })
}

/// Full schema object (`{ groups: [...] }`); null members are kept.
pub fn get() -> Value {
    let slugs = sorted_slugs();
    let groups = vec![
        group("server", "Сервер", Some("Прослушивание и ключи доступа"), vec![
            fd("listenip", "string", "IP прослушивания", Some("any или конкретный IP")),
            field("listenport", "int", "Порт", Some("1-65535"), F { min: Some(1), max: Some(65535), ..F::default() }),
            field("apikey", "password", "API ключ", Some("Пусто - без проверки"), secret()),
            field("devkey", "password", "Dev ключ", Some("Пароль входа в админ-панель; ключ для /dev/, /cron/, /jsondb из интернета и через туннель"), secret()),
            fd("web", "bool", "Веб-интерфейс", Some("Раздавать сайт и документацию из wwwroot/ (админ-панель включается отдельно: admin.enable)")),
        ]),
        group("api", "API и дубликаты", None, vec![
            fd("openstats", "bool", "Открытая статистика", None),
            fd("opensync", "bool", "Открытый sync", None),
            fd("mergeduplicates", "bool", "Объединять дубликаты", None),
            fd("mergenumduplicates", "bool", "Объединять по номеру", Some("Серии и т.п.")),
        ]),
        group("sync", "Синхронизация", None, vec![
            fd("syncapi", "string", "Sync API URL", Some("URL удалённого инстанса")),
            field("synctrackers", "stringList", "Sync трекеры", Some("Трекеры для синхронизации с удалённым инстансом"), enums(&slugs)),
            field("disable_trackers", "stringList", "Отключённые трекеры", Some("Трекеры, которые не должны работать на этом инстансе"), enums(&slugs)),
            fd("syncsport", "bool", "Sync sport", None),
            fd("syncspidr", "bool", "Sync spidr", None),
            field("timeSync", "int", "Интервал sync (мин)", None, min(1)),
            field("timeSyncSpidr", "int", "Интервал sync spidr (мин)", None, min(1)),
            field("saveCheckpointEveryNBatches", "int", "Sync checkpoint (батчей)", Some("При catch-up: сохранять masterDb каждые N батчей; 0 - только по таймеру (5 мин)"), min(0)),
            field("maxreadfile", "int", "Max read file", Some("Лимит чтения fdb"), min(1)),
        ]),
        group("logging", "Логирование", Some("Файлы в Data/log/ и уровни консоли (journalctl)"), vec![
            fd("logFdb", "bool", "Журнал изменений базы", Some("Data/log/fdb.*.log: строка на каждое изменение раздачи. Много записи на диск, включайте для отладки. По умолчанию выключен")),
            field("logFdbRetentionDays", "int", "Хранение fdb логов (дней)", Some("0 - все"), min(0)),
            field("logFdbMaxSizeMb", "int", "Max размер fdb логов (MB)", Some("Старые файлы удаляются при превышении; 0 - без лимита. По умолчанию 1024"), min(0)),
            field("logFdbMaxFiles", "int", "Max файлов fdb логов", Some("0 - без лимита"), min(0)),
            fd("logParsers", "bool", "Лог парсеров", Some("Data/log/{tracker}.log, default: true")),
            field("logging.defaultLevel", "select", "Уровень консоли", Some("Минимальный уровень для journalctl"), enums(&["Trace", "Debug", "Information", "Warning", "Error", "Critical", "None"])),
            fd("logging.consoleTimestamp", "bool", "Время в консоли", Some("Дублировать timestamp в строке сообщения")),
            fd("logging.tracksConsoleDetail", "bool", "Подробный tracks в консоли", Some("false - только ошибки и итоги")),
            field("logging.cronSkipFastMs", "int", "Cron: быстрые 200 → Debug", Some("HTTP /cron/ быстрее N ms, 0 - логировать все"), min(0)),
            fd("logging.categories", "json", "Уровни по категориям", Some("JSON: tracks, sync, sync_spidr, cron, fdb, stats, parsers (None = выкл.)")),
        ]),
        group("tracks", "Tracks (ffprobe)", None, vec![
            fd("tracks", "bool", "Включить tracks", Some("Сбор метаданных через tsuri")),
            fd("trackslog", "bool", "Лог tracks", Some("Data/log/tracks.log, default: true")),
            fd("trackscategory", "string", "Категория tracks", Some("Уникально для инстанса")),
            field("tracksdelay", "int", "Задержка tsuri (мс)", None, min(0)),
            field("tracksatempt", "int", "Попыток tracks", None, min(1)),
            field("tracksconcurrency", "int", "Параллельных анализов", Some("Глобальный лимит к tsuri"), min(1)),
            field("tracksffptimeout", "int", "Таймаут /ffp (сек)", Some("При sid > 0"), min(1)),
            field("tracksffptimeoutnosid", "int", "Таймаут /ffp без sid (сек)", Some("При sid == 0"), min(1)),
            field("tracksreadtimeout", "int", "Ожидание file_stats (сек)", Some("WaitTorrentReady"), min(1)),
            field("trackspeerwaittimeout", "int", "Проверка сидов (сек)", Some("До вызова /ffp"), min(1)),
            field("tracksffpretry", "int", "Доп. file id на попытку", Some("/ffp retry"), min(0)),
            field("tracksminbufferkb", "int", "Мин. буфер перед /ffp (KB)", None, min(0)),
            field("tracksorphansweepmin", "int", "Sweep сирот (мин)", Some("rem в trackscategory"), min(1)),
            field("tracksmod", "select", "Режим tracks", Some("0 - все, 1 - за сутки"), enums(&["0", "1"])),
            field("tracksinterval.task0", "int", "Tracks task0 (мин)", Some("Все задачи"), min(1)),
            field("tracksinterval.task1", "int", "Tracks task1 (мин)", Some("За сутки"), min(1)),
            fd("tsuri", "stringList", "TSURI", Some("URL сервисов ffprobe, по одному на строку")),
        ]),
        group("fdb", "FileDB", None, vec![
            field("fdbPathLevels", "int", "Уровни fdb", Some("1-4"), F { min: Some(1), max: Some(4), ..F::default() }),
            field("timeStatsUpdate", "int", "Обновление stats (мин)", None, min(1)),
        ]),
        group("evercache", "Evercache", None, vec![
            fd("evercache.enable", "bool", "Включить", None),
            field("evercache.validHour", "int", "Valid hour", Some("0 - бессрочно"), min(0)),
            field("evercache.maxOpenWriteTask", "int", "Max open write", None, min(1)),
            field("evercache.dropCacheTake", "int", "Drop cache take", None, min(1)),
        ]),
        group("search", "Поиск (combined)", Some("Jackett JSON + Torznab combined search"), vec![
            field("search.mergeV1", "select", "Merge v1 (fuzzy)", Some("auto - только fuzzy; card без v1"), enums(&["false", "auto", "true"])),
            field("search.maxV1Pairs", "int", "Max v1 pairs", Some("При mergeV1=auto или true (fuzzy)"), min(1)),
            fd("search.v1Sort", "string", "V1 sort", Some("sid, pir, size…")),
            fd("search.stripTrailingYear", "bool", "Strip trailing year", Some("Fuzzy: запрос без года")),
            fd("search.stripSeasonEpisode", "bool", "Strip season/episode", Some("Fuzzy: запрос без SxxExx")),
            fd("search.skipSeasonEpisodeFilter", "bool", "Skip season/ep filter", Some("Не фильтровать season/ep на сервере")),
            fd("search.skipCatFilter", "bool", "Skip cat filter", Some("Не фильтровать cat/Category[] на сервере")),
        ]),
        group("alloha", "Alloha", Some("KP/IMDB/TMDB ID → title (API v2)"), vec![
            fd("alloha.enable", "bool", "Включить", Some("Резолв tt… / kp… / tmdb… через Alloha")),
            fd("alloha.baseUrl", "string", "Base URL", Some("https://apbugall.org")),
            field("alloha.token", "password", "Bearer token", Some("Authorization: Bearer …"), secret()),
            field("alloha.timeoutSeconds", "int", "Timeout (с)", None, min(1)),
            field("alloha.cacheHours", "int", "Cache (ч)", Some("Memory cache ID → titles"), min(0)),
            fd("alloha.filterByYear", "bool", "Filter by year", Some("Если клиент не передал year, ±1 от Alloha")),
        ]),
        group("torznab", "Torznab", Some("Torznab XML (Sonarr/Radarr/Prowlarr)"), vec![
            fd("torznab.enable", "bool", "Torznab XML", Some("/torznab/api и Torznab-алиасы")),
            fd("torznab.enrichTitles", "bool", "Enrich titles", Some("Озвучки в XML title")),
        ]),
        group("proxy", "Прокси", None, vec![
            fd("proxy.pattern", "string", "Pattern", Some("Regex для proxy")),
            fd("proxy.useAuth", "bool", "Use auth", None),
            fd("proxy.BypassOnLocal", "bool", "Bypass on local", None),
            field("proxy.username", "password", "Username", None, secret()),
            field("proxy.password", "password", "Password", None, secret()),
            fd("proxy.list", "stringList", "Proxy list", Some("ip:port или socks5://…")),
            fd("globalproxy", "json", "Global proxy", Some("JSON-массив ProxySettings")),
        ]),
        group("flaresolverr", "FlareSolverr", Some("Cloudflare bypass: отдельная сессия Chromium на хост (Rutracker, Kinozal)"), vec![
            fd("flaresolverr.enable", "bool", "Включить", Some("Ходить на CF-хосты через браузер")),
            fd("flaresolverr.url", "string", "URL", Some("http://127.0.0.1:8191/v1 или http://flaresolverr:8191/v1")),
            fd("flaresolverr.crawlUrl", "string", "Crawl URL", Some("Второй FlareSolverr для ParseAll/UpdateTasks. Пусто - тот же url")),
            field("flaresolverr.maxTimeoutMs", "int", "Таймаут (мс)", Some("Первая страница / challenge+retry (~5 мин)"), min(1000)),
            field("flaresolverr.sessionIdleMinutes", "int", "Idle сессии (мин)", Some("Закрыть Chromium после простоя (дефолт 120; keep-alive cron чаще)"), min(0)),
            field("flaresolverr.browserTimeoutRetries", "int", "Retry на timeout", Some("Same-session retry до recycle (дефолт 1)"), min(0)),
            field("flaresolverr.recycleAfterTimeouts", "int", "Recycle после N timeout", Some("Destroy сессии после N подряд browser timeout (дефолт 3)"), min(1)),
            field("flaresolverr.guardedHours", "int", "Guarded hours", Some("Сколько помнить CF на хосте"), min(1)),
            field("flaresolverr.recheckMinutes", "int", "Recheck (мин)", Some("Как часто пробовать обычный GET"), min(1)),
        ]),
        group("cffetch", "cffetch", Some("Быстрый путь после CF: ghcr.io/jacred-fdb/cffetch на :8192, тот же SOCKS что у FlareSolverr"), vec![
            fd("cffetch.enable", "bool", "Включить", Some("После solve ходить без page.goto")),
            fd("cffetch.url", "string", "URL", Some("http://127.0.0.1:8192/fetch")),
            fd("cffetch.impersonate", "string", "Impersonate", Some("chrome136, не старше Chromium FlareSolverr")),
            field("cffetch.timeoutSeconds", "int", "Таймаут (с)", Some("Таймаут помощника"), min(5)),
            field("cffetch.maxConcurrent", "int", "Одновременно", Some("Без лимита будет 429 трекера"), min(1)),
            field("cffetch.clearanceMinutes", "int", "Cookie (мин)", Some("Потом отказ уводит на браузер"), min(0)),
            fd("cffetch.proxy", "string", "SOCKS", Some("socks5://127.0.0.1:20001 как PROXY_URL у FlareSolverr")),
        ]),
        group("admin", "Админ-панель", Some("Вход: {path}?{token}, затем пароль (devkey). После смены пути или токена откройте новый адрес"), vec![
            fd("admin.enable", "bool", "Включить", Some("false - панель недоступна (404)")),
            fd("admin.path", "string", "Путь", Some("Один сегмент [a-z0-9_-], 2-32 символа, например /admin")),
            field("admin.token", "password", "Токен входа", Some("[A-Za-z0-9], 12-64 символа; генерируется при первом запуске"), secret()),
            field("admin.sessionHours", "int", "Сессия (ч)", Some("Время жизни сессии после входа"), F { min: Some(1), max: Some(8760), ..F::default() }),
        ]),
        group("waf", "WAF", Some("Фильтр запросов: встроенные домены → whitelist → blacklist/баны → домены → User-Agent → ловушки → rate limit. Списки и баны - в разделе WAF (Data/waf.json)"), vec![
            fd("waf.enable", "bool", "Включить", Some("false - запросы не блокируются (статистика ведётся при logRequests)")),
            fd("waf.logRequests", "bool", "Журнал запросов", Some("Журнал и статистика в памяти, сбрасываются при перезапуске")),
            field("waf.historySize", "int", "Размер журнала", Some("Последних запросов в памяти"), F { min: Some(0), max: Some(MAX_WAF_HISTORY), ..F::default() }),
            fd("waf.whitelistLan", "bool", "LAN без ограничений", Some("LAN/loopback никогда не ограничиваются и не банятся (loopback - всегда)")),
            fd("waf.rateLimit.enable", "bool", "Rate limit", None),
            field("waf.rateLimit.perMinute", "int", "Запросов/мин с IP", Some("Скользящая минута; превышение → 429 и бан"), min(1)),
            field("waf.rateLimit.banMinutes", "int", "Бан за превышение (мин)", Some("Также бан за User-Agent"), min(1)),
            fd("waf.trapPaths", "stringList", "Ловушки", Some("Префиксы путей (без учёта регистра): 404 и бан, например /.env, /wp-admin")),
            field("waf.trapBanMinutes", "int", "Бан за ловушку (мин)", None, min(1)),
            fd("waf.blockUserAgents", "stringList", "Блок User-Agent", Some("Regex без учёта регистра, по одному на строку: sqlmap, nikto, masscan")),
            fd("waf.domainAllowlistOnly", "bool", "Только разрешённые домены", Some("Запросы с Origin/Referer не из белого списка доменов и не с этого сервера → 403. Без Origin/Referer - пропускаются")),
        ]),
        tracker_groups(),
    ];
    json!({ "groups": groups })
}

/// Upper bound of `waf.historySize`.
pub const MAX_WAF_HISTORY: i32 = 100_000;

fn validate_waf(w: &crab_core::config::WafSettings, errors: &mut Vec<String>, warnings: &mut Vec<String>) {
    if !(0..=MAX_WAF_HISTORY).contains(&w.historySize) {
        errors.push(format!("waf.historySize: значение должно быть от 0 до {MAX_WAF_HISTORY}"));
    }
    if w.rateLimit.perMinute < 1 {
        errors.push("waf.rateLimit.perMinute: должно быть ≥ 1".into());
    }
    if w.rateLimit.banMinutes < 1 {
        errors.push("waf.rateLimit.banMinutes: должно быть ≥ 1".into());
    }
    if w.trapBanMinutes < 1 {
        errors.push("waf.trapBanMinutes: должно быть ≥ 1".into());
    }
    for p in &w.blockUserAgents {
        if p.trim().is_empty() {
            errors.push("waf.blockUserAgents: пустой regex".into());
        } else if let Err(e) = regex::RegexBuilder::new(p).case_insensitive(true).size_limit(1 << 20).build() {
            let first = e.to_string().lines().last().unwrap_or("").trim().to_string();
            errors.push(format!("waf.blockUserAgents: некорректный regex «{p}»: {first}"));
        }
    }
    for p in &w.trapPaths {
        if !p.trim().is_empty() && !p.trim().starts_with('/') {
            warnings.push(format!("waf.trapPaths: «{p}» должен начинаться с /"));
        }
    }
    if w.domainAllowlistOnly && crate::waf::WAF.domain_whitelist_len(chrono::Utc::now()) == 0 {
        warnings.push("waf.domainAllowlistOnly: белый список доменов пуст - браузерные запросы с других сайтов получат 403 (WAF → Правила → Домены)".into());
    }
}

fn valid_log_level(v: &str) -> bool {
    let v = v.trim();
    ["trace", "debug", "information", "warning", "error", "critical", "none"].contains(&v.to_ascii_lowercase().as_str())
        || v.parse::<i64>().is_ok()
}

fn validate_tracker_list(list: Option<&[String]>, field: &str, warnings: &mut Vec<String>) {
    for item in list.unwrap_or(&[]) {
        if item.trim().is_empty() {
            continue;
        }
        if !is_known_tracker(item) {
            warnings.push(format!("{field}: неизвестный трекер «{item}»"));
        }
    }
}

/// Model-level validation; appends messages to `errors` / `warnings`.
pub fn validate_against_schema(c: &AppOptions, errors: &mut Vec<String>, warnings: &mut Vec<String>) {
    let mut err = |cond: bool, msg: &str| {
        if cond {
            errors.push(msg.to_string());
        }
    };

    err(c.listenport < 1, "listenport: значение должно быть от 1 до 65535");
    if !c.listenip.is_empty() && !c.listenip.eq_ignore_ascii_case("any") && c.listenip.parse::<std::net::IpAddr>().is_err() {
        warnings.push("listenip: ожидается 'any' или корректный IP-адрес".into());
    }
    err(c.tracksmod != 0 && c.tracksmod != 1, "tracksmod: допустимы только 0 или 1");
    err(c.timeSync < 1, "timeSync: должно быть ≥ 1");
    err(c.saveCheckpointEveryNBatches < 0, "saveCheckpointEveryNBatches: не может быть отрицательным");
    err(c.timeStatsUpdate < 1, "timeStatsUpdate: должно быть ≥ 1");
    err(c.maxreadfile < 1, "maxreadfile: должно быть ≥ 1");
    if c.fdbPathLevels < 1 || c.fdbPathLevels > 4 {
        warnings.push("fdbPathLevels: рекомендуется 1-4".into());
    }
    err(c.logFdbRetentionDays < 0, "logFdbRetentionDays: не может быть отрицательным");
    err(c.tracksdelay < 0, "tracksdelay: не может быть отрицательным");
    err(c.tracksatempt < 1, "tracksatempt: должно быть ≥ 1");
    err(c.tracksconcurrency < 1, "tracksconcurrency: должно быть ≥ 1");
    err(c.tracksffptimeout < 1, "tracksffptimeout: должно быть ≥ 1");
    err(c.tracksffptimeoutnosid < 1, "tracksffptimeoutnosid: должно быть ≥ 1");
    err(c.tracksreadtimeout < 1, "tracksreadtimeout: должно быть ≥ 1");
    err(c.trackspeerwaittimeout < 1, "trackspeerwaittimeout: должно быть ≥ 1");
    err(c.tracksffpretry < 0, "tracksffpretry: не может быть отрицательным");
    err(c.tracksminbufferkb < 0, "tracksminbufferkb: не может быть отрицательным");
    err(c.tracksorphansweepmin < 1, "tracksorphansweepmin: должно быть ≥ 1");

    validate_tracker_list(c.synctrackers.as_deref(), "synctrackers", warnings);
    validate_tracker_list(Some(&c.disable_trackers), "disable_trackers", warnings);

    for uri in &c.tsuri {
        if uri.trim().is_empty() {
            continue;
        }
        let ok = url::Url::parse(uri).map(|u| u.scheme() == "http" || u.scheme() == "https").unwrap_or(false);
        if !ok {
            warnings.push(format!("tsuri: некорректный URL «{uri}»"));
        }
    }

    let mv = c.search.mergeV1.to_lowercase();
    if mv != "auto" && mv != "true" && mv != "false" {
        errors.push("search.mergeV1: допустимы auto, true, false".into());
    }

    if c.logging.cronSkipFastMs < 0 {
        errors.push("logging.cronSkipFastMs: не может быть отрицательным".into());
    }
    if !c.logging.defaultLevel.trim().is_empty() && !valid_log_level(&c.logging.defaultLevel) {
        warnings.push("logging.defaultLevel: ожидается Trace, Debug, Information, Warning, Error, Critical или None".into());
    }

    if c.evercache.validHour < 0 {
        errors.push("evercache.validHour: не может быть отрицательным".into());
    }
    if c.evercache.maxOpenWriteTask < 1 {
        errors.push("evercache.maxOpenWriteTask: должно быть ≥ 1".into());
    }

    if let Err(e) = crab_core::config::normalize_admin_path(&c.admin.path) {
        errors.push(e);
    }
    if c.admin.token.is_empty() {
        warnings.push("admin.token: пусто - панель недоступна до перезапуска (токен будет сгенерирован)".into());
    } else if !crab_core::config::is_valid_admin_token(&c.admin.token) {
        errors.push("admin.token: допустимы только A-Z, a-z, 0-9, длина 12-64".into());
    }
    if !(1..=8760).contains(&c.admin.sessionHours) {
        errors.push("admin.sessionHours: значение должно быть от 1 до 8760".into());
    }

    validate_waf(&c.waf, errors, warnings);

    if c.TracksInterval.task0 < 1 {
        errors.push("tracksinterval.task0: должно быть ≥ 1".into());
    }
    if c.TracksInterval.task1 < 1 {
        errors.push("tracksinterval.task1: должно быть ≥ 1".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_group_and_rules() {
        let s = get();
        let g = s["groups"].as_array().unwrap().iter().find(|g| g["id"] == "admin").unwrap().clone();
        let keys: Vec<&str> = g["fields"].as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["admin.enable", "admin.path", "admin.token", "admin.sessionHours"]);
        assert_eq!(g["fields"][2]["sensitive"], true);

        let mut c = AppOptions::default();
        c.admin.path = "/api".into();
        c.admin.token = "bad token!".into();
        c.admin.sessionHours = 0;
        let (mut e, mut w) = (vec![], vec![]);
        validate_against_schema(&c, &mut e, &mut w);
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(e[0].starts_with("admin.path"));
        let (mut e, mut w) = (vec![], vec![]);
        validate_against_schema(&AppOptions::default(), &mut e, &mut w);
        assert!(e.is_empty());
        assert!(w[0].starts_with("admin.token"));
    }

    #[test]
    fn waf_group_and_rules() {
        let s = get();
        let g = s["groups"].as_array().unwrap().iter().find(|g| g["id"] == "waf").unwrap().clone();
        let keys: Vec<&str> = g["fields"].as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap()).collect();
        assert!(keys.contains(&"waf.rateLimit.perMinute") && keys.contains(&"waf.blockUserAgents"));

        let mut c = AppOptions::default();
        c.admin.token = "Z0mt0N7r2hoUM2TuOk".into();
        c.waf.rateLimit.perMinute = 0;
        c.waf.rateLimit.banMinutes = 0;
        c.waf.trapBanMinutes = 0;
        c.waf.historySize = -1;
        c.waf.blockUserAgents = vec!["sqlmap".into(), "(unclosed".into(), " ".into()];
        c.waf.trapPaths = vec!["wp-admin".into()];
        let (mut e, mut w) = (vec![], vec![]);
        validate_against_schema(&c, &mut e, &mut w);
        assert_eq!(e.len(), 6, "{e:?}");
        assert!(e[0].starts_with("waf.historySize"));
        assert!(e[1].starts_with("waf.rateLimit.perMinute"));
        assert!(e[4].starts_with("waf.blockUserAgents: некорректный regex «(unclosed»"), "{e:?}");
        assert_eq!(w, vec!["waf.trapPaths: «wp-admin» должен начинаться с /"]);
    }

    #[test]
    fn schema_shape() {
        let s = get();
        let groups = s["groups"].as_array().unwrap();
        assert_eq!(groups[0]["id"], "server");
        assert!(groups[1]["description"].is_null());
        assert!(groups[0]["fields"][0].as_object().unwrap().contains_key("min"));
        assert_eq!(groups[0]["fields"][1]["max"], 65535);
        let trackers = groups.last().unwrap()["trackers"].as_array().unwrap();
        assert_eq!(trackers.len(), 25);
        assert_eq!(trackers[0]["id"], "Anibelka");
        assert_eq!(trackers[15]["trackerSlug"], "nnmclub");
        assert_eq!(groups[2]["fields"][1]["enumValues"][0], "anibelka");
    }

    #[test]
    fn defaults_are_valid() {
        let (mut e, mut w) = (vec![], vec![]);
        let mut c = AppOptions::default();
        c.admin.token = "Z0mt0N7r2hoUM2TuOk".into();
        validate_against_schema(&c, &mut e, &mut w);
        assert!(e.is_empty(), "{e:?}");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn detects_errors_and_warnings() {
        let mut c = AppOptions::default();
        c.tracksmod = 3;
        c.listenip = "nope".into();
        c.disable_trackers = vec!["foo".into(), "Rutor".into()];
        c.search.mergeV1 = "maybe".into();
        c.tsuri = vec!["ftp://x".into()];
        c.admin.token = "Z0mt0N7r2hoUM2TuOk".into();
        let (mut e, mut w) = (vec![], vec![]);
        validate_against_schema(&c, &mut e, &mut w);
        assert_eq!(e, vec!["tracksmod: допустимы только 0 или 1", "search.mergeV1: допустимы auto, true, false"]);
        assert_eq!(
            w,
            vec![
                "listenip: ожидается 'any' или корректный IP-адрес",
                "disable_trackers: неизвестный трекер «foo»",
                "tsuri: некорректный URL «ftp://x»"
            ]
        );
    }
}
