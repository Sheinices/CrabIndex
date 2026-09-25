//! Rutracker forum map: forum id → types, title layout and hourly quick-parse flag.

use indexmap::IndexMap;
use once_cell::sync::Lazy;

/// How listing titles are laid out in a forum (drives name/originalname/year extraction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleKind {
    Movie,
    Serial,
    NonStandard,
}

#[derive(Clone, Debug)]
pub struct Category {
    pub types: &'static [&'static str],
    pub title_kind: TitleKind,
    /// Included in the hourly first-page parse. The full map is used by UpdateTasksParse.
    pub quick_parse: bool,
}

type Row = (&'static str, &'static [&'static str], TitleKind, bool);

#[rustfmt::skip]
const ROWS: &[Row] = &[
    // 3D Кинофильмы / Наше / Зарубежное / Арт-хаус / HD Video (movie)
    ("549", &["movie"], TitleKind::Movie, true),
    ("22", &["movie"], TitleKind::Movie, true),
    ("1666", &["movie"], TitleKind::Movie, true),
    ("941", &["movie"], TitleKind::Movie, true),
    ("1950", &["movie"], TitleKind::Movie, true),
    ("2090", &["movie"], TitleKind::Movie, true),
    ("2221", &["movie"], TitleKind::Movie, true),
    ("2091", &["movie"], TitleKind::Movie, true),
    ("2092", &["movie"], TitleKind::Movie, true),
    ("2093", &["movie"], TitleKind::Movie, true),
    ("2200", &["movie"], TitleKind::Movie, true),
    ("2540", &["movie"], TitleKind::Movie, true),
    ("934", &["movie"], TitleKind::Movie, true),
    ("505", &["movie"], TitleKind::Movie, true),
    ("124", &["movie"], TitleKind::Movie, true),
    ("1457", &["movie"], TitleKind::Movie, true),
    ("2199", &["movie"], TitleKind::Movie, true),
    ("313", &["movie"], TitleKind::Movie, true),
    ("312", &["movie"], TitleKind::Movie, true),
    ("1247", &["movie"], TitleKind::Movie, true),
    ("2201", &["movie"], TitleKind::Movie, true),
    ("2339", &["movie"], TitleKind::Movie, true),
    ("140", &["movie"], TitleKind::Movie, true),
    ("252", &["movie"], TitleKind::Movie, true),

    // Фильмы: разделы UHD и DVD, тоже пропущенные (15.08.2026).
    ("7", &["movie"], TitleKind::Movie, true),
    ("718", &["movie"], TitleKind::Movie, true),
    ("1940", &["movie"], TitleKind::Movie, true),
    ("271", &["movie"], TitleKind::Movie, true),
    ("272", &["movie"], TitleKind::Movie, true),
    ("775", &["movie"], TitleKind::Movie, true),
    ("1543", &["movie"], TitleKind::Movie, true),
    ("101", &["movie"], TitleKind::Movie, true),
    ("100", &["movie"], TitleKind::Movie, true),
    ("572", &["movie"], TitleKind::Movie, true),

    // 3D Мультфильмы / Мультфильмы
    ("84", &["multfilm"], TitleKind::Movie, true),
    ("2343", &["multfilm"], TitleKind::Movie, true),
    ("930", &["multfilm"], TitleKind::Movie, true),
    ("2365", &["multfilm"], TitleKind::Movie, true),
    ("208", &["multfilm"], TitleKind::Movie, true),
    ("539", &["multfilm"], TitleKind::Movie, true),
    ("209", &["multfilm"], TitleKind::Movie, true),
    ("1213", &["multfilm"], TitleKind::Movie, true),
    ("4", &["multfilm"], TitleKind::Movie, true),
    ("1577", &["multfilm"], TitleKind::Movie, true),

    // Мультсериалы
    ("921", &["multserial"], TitleKind::Serial, true),
    ("815", &["multserial"], TitleKind::Serial, true),
    ("1460", &["multserial"], TitleKind::Serial, true),
    ("498", &["multserial"], TitleKind::Serial, true),

    // Сериалы (зарубежные, русские, HD, LatAm, KR/JP)
    ("842", &["serial"], TitleKind::Serial, true),
    ("235", &["serial"], TitleKind::Serial, true),
    ("242", &["serial"], TitleKind::Serial, true),
    ("819", &["serial"], TitleKind::Serial, true),
    ("1531", &["serial"], TitleKind::Serial, true),
    ("721", &["serial"], TitleKind::Serial, true),
    ("1102", &["serial"], TitleKind::Serial, true),
    ("1120", &["serial"], TitleKind::Serial, true),
    ("1214", &["serial"], TitleKind::Serial, true),
    ("489", &["serial"], TitleKind::Serial, true),
    ("387", &["serial"], TitleKind::Serial, true),
    ("9", &["serial"], TitleKind::Serial, true),
    ("81", &["serial"], TitleKind::Serial, true),
    ("119", &["serial"], TitleKind::Serial, true),
    ("1803", &["serial"], TitleKind::Serial, true),
    ("266", &["serial"], TitleKind::Serial, true),
    ("193", &["serial"], TitleKind::Serial, true),
    ("1690", &["serial"], TitleKind::Serial, true),
    ("1459", &["serial"], TitleKind::Serial, true),
    ("1463", &["serial"], TitleKind::Serial, true),
    ("825", &["serial"], TitleKind::Serial, true),
    ("1248", &["serial"], TitleKind::Serial, true),
    ("1288", &["serial"], TitleKind::Serial, true),
    ("325", &["serial"], TitleKind::Serial, true),
    ("534", &["serial"], TitleKind::Serial, true),
    ("694", &["serial"], TitleKind::Serial, true),
    ("704", &["serial"], TitleKind::Serial, true),
    ("915", &["serial"], TitleKind::NonStandard, true),
    ("1939", &["serial"], TitleKind::NonStandard, true),

    // Сериальные разделы, которых в списке не было (добавлены 15.08.2026).
    //
    // Повод: раздача «Дом дракона» S3 в 2160p (t=6873874) не находилась
    // ни в вебе, ни в Лампе - она лежит в разделе 1171, а у нас был
    // только его РОДИТЕЛЬ, 119. У rutracker раздел может иметь и
    // подразделы, и собственные темы, поэтому обход родителя не
    // заменяет обход детей: в самом 119 всего два десятка тем.
    ("2366", &["serial"], TitleKind::Serial, true),
    ("189", &["serial"], TitleKind::Serial, true),
    ("1171", &["serial"], TitleKind::Serial, true),
    ("812", &["serial"], TitleKind::Serial, true),
    ("920", &["serial"], TitleKind::Serial, true),
    ("911", &["serial"], TitleKind::Serial, true),
    ("2100", &["serial"], TitleKind::Serial, true),

    // Географические UHD/HD-листы под 119 / 2100 / 2366 (добавлены 10.09.2026).
    //
    // Повод: Silo S2 UHD (t=6601495) уехала из 2366 в 1669 «Сериалы США и
    // Канады (UHD Video)». Родитель 119 в карте был, но его viewforum
    // не содержит тем детей. Jackett-имена для этих id устарели
    // (переиспользование разделов) - подписи брать с живого сайта.
    ("1669", &["serial"], TitleKind::Serial, true),
    ("2393", &["serial"], TitleKind::Serial, true),
    ("625", &["serial"], TitleKind::Serial, true),
    ("1949", &["serial"], TitleKind::Serial, true),
    ("173", &["serial"], TitleKind::Serial, true),
    ("820", &["serial"], TitleKind::NonStandard, true),
    ("1242", &["serial"], TitleKind::NonStandard, true),
    ("717", &["serial"], TitleKind::NonStandard, true),
    ("2412", &["serial"], TitleKind::NonStandard, true),

    // Аниме
    ("1105", &["anime"], TitleKind::NonStandard, true),
    ("2491", &["anime"], TitleKind::NonStandard, true),
    ("1389", &["anime"], TitleKind::NonStandard, true),
    ("33", &["anime"], TitleKind::NonStandard, true),
    ("1106", &["anime"], TitleKind::NonStandard, true),

    // Документальные фильмы
    ("709", &["documovie"], TitleKind::Movie, false),
    ("2109", &["documovie"], TitleKind::Movie, false),
    ("1985", &["documovie"], TitleKind::Movie, false),
    ("1202", &["documovie"], TitleKind::Movie, false),

    // Документалистика
    ("46", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("671", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2177", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2538", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("251", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("98", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("97", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("851", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2178", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("821", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2076", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("56", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2123", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("876", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2139", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("1467", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("1469", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("249", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("552", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("500", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2112", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("1327", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("1468", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2168", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2160", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("314", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("1281", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2110", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("979", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2169", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2164", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2166", &["docuserial", "documovie"], TitleKind::NonStandard, false),
    ("2163", &["docuserial", "documovie"], TitleKind::NonStandard, false),

    // Развлекательные телепередачи и шоу
    ("24", &["tvshow"], TitleKind::NonStandard, false),
    ("1959", &["tvshow"], TitleKind::NonStandard, false),
    ("939", &["tvshow"], TitleKind::NonStandard, false),
    ("1481", &["tvshow"], TitleKind::NonStandard, false),
    ("113", &["tvshow"], TitleKind::NonStandard, false),
    ("115", &["tvshow"], TitleKind::NonStandard, false),
    ("882", &["tvshow"], TitleKind::NonStandard, false),
    ("1482", &["tvshow"], TitleKind::NonStandard, false),
    ("393", &["tvshow"], TitleKind::NonStandard, false),
    ("2537", &["tvshow"], TitleKind::NonStandard, false),
    ("532", &["tvshow"], TitleKind::NonStandard, false),
    ("827", &["tvshow"], TitleKind::NonStandard, false),

    // Спорт
    ("1392", &["sport"], TitleKind::NonStandard, false),
    ("2475", &["sport"], TitleKind::NonStandard, false),
    ("2493", &["sport"], TitleKind::NonStandard, false),
    ("2113", &["sport"], TitleKind::NonStandard, false),
    ("2482", &["sport"], TitleKind::NonStandard, false),
    ("2103", &["sport"], TitleKind::NonStandard, false),
    ("2522", &["sport"], TitleKind::NonStandard, false),
    ("2485", &["sport"], TitleKind::NonStandard, false),
    ("2486", &["sport"], TitleKind::NonStandard, false),
    ("2479", &["sport"], TitleKind::NonStandard, false),
    ("2089", &["sport"], TitleKind::NonStandard, false),
    ("1794", &["sport"], TitleKind::NonStandard, false),
    ("845", &["sport"], TitleKind::NonStandard, false),
    ("2312", &["sport"], TitleKind::NonStandard, false),
    ("343", &["sport"], TitleKind::NonStandard, false),
    ("2111", &["sport"], TitleKind::NonStandard, false),
    ("1527", &["sport"], TitleKind::NonStandard, false),
    ("2069", &["sport"], TitleKind::NonStandard, false),
    ("1323", &["sport"], TitleKind::NonStandard, false),
    ("2009", &["sport"], TitleKind::NonStandard, false),
    ("2010", &["sport"], TitleKind::NonStandard, false),
    ("2006", &["sport"], TitleKind::NonStandard, false),
    ("2007", &["sport"], TitleKind::NonStandard, false),
    ("2005", &["sport"], TitleKind::NonStandard, false),
    ("259", &["sport"], TitleKind::NonStandard, false),
    ("2004", &["sport"], TitleKind::NonStandard, false),
    ("2001", &["sport"], TitleKind::NonStandard, false),
    ("2002", &["sport"], TitleKind::NonStandard, false),
    ("283", &["sport"], TitleKind::NonStandard, false),
    ("1997", &["sport"], TitleKind::NonStandard, false),
    ("2003", &["sport"], TitleKind::NonStandard, false),
    ("1608", &["sport"], TitleKind::NonStandard, false),
    ("2294", &["sport"], TitleKind::NonStandard, false),
    ("1229", &["sport"], TitleKind::NonStandard, false),
    ("1693", &["sport"], TitleKind::NonStandard, false),
    ("2532", &["sport"], TitleKind::NonStandard, false),
    ("136", &["sport"], TitleKind::NonStandard, false),
    ("592", &["sport"], TitleKind::NonStandard, false),
    ("2533", &["sport"], TitleKind::NonStandard, false),
    ("1952", &["sport"], TitleKind::NonStandard, false),
    ("1621", &["sport"], TitleKind::NonStandard, false),
    ("2075", &["sport"], TitleKind::NonStandard, false),
    ("1668", &["sport"], TitleKind::NonStandard, false),
    ("1613", &["sport"], TitleKind::NonStandard, false),
    ("1614", &["sport"], TitleKind::NonStandard, false),
    ("1623", &["sport"], TitleKind::NonStandard, false),
    ("1615", &["sport"], TitleKind::NonStandard, false),
    ("1630", &["sport"], TitleKind::NonStandard, false),
    ("2425", &["sport"], TitleKind::NonStandard, false),
    ("2514", &["sport"], TitleKind::NonStandard, false),
    ("1616", &["sport"], TitleKind::NonStandard, false),
    ("2014", &["sport"], TitleKind::NonStandard, false),
    ("1442", &["sport"], TitleKind::NonStandard, false),
    ("1491", &["sport"], TitleKind::NonStandard, false),
    ("1987", &["sport"], TitleKind::NonStandard, false),
    ("1617", &["sport"], TitleKind::NonStandard, false),
    ("1620", &["sport"], TitleKind::NonStandard, false),
    ("1998", &["sport"], TitleKind::NonStandard, false),
    ("1343", &["sport"], TitleKind::NonStandard, false),
    ("751", &["sport"], TitleKind::NonStandard, false),
    ("1697", &["sport"], TitleKind::NonStandard, false),
    ("255", &["sport"], TitleKind::NonStandard, false),
    ("260", &["sport"], TitleKind::NonStandard, false),
    ("256", &["sport"], TitleKind::NonStandard, false),
    ("1986", &["sport"], TitleKind::NonStandard, false),
    ("660", &["sport"], TitleKind::NonStandard, false),
    ("1551", &["sport"], TitleKind::NonStandard, false),
    ("626", &["sport"], TitleKind::NonStandard, false),
    ("262", &["sport"], TitleKind::NonStandard, false),
    ("1326", &["sport"], TitleKind::NonStandard, false),
    ("978", &["sport"], TitleKind::NonStandard, false),
    ("1287", &["sport"], TitleKind::NonStandard, false),
    ("1188", &["sport"], TitleKind::NonStandard, false),
    ("1667", &["sport"], TitleKind::NonStandard, false),
    ("1675", &["sport"], TitleKind::NonStandard, false),
    ("257", &["sport"], TitleKind::NonStandard, false),
    ("875", &["sport"], TitleKind::NonStandard, false),
    ("263", &["sport"], TitleKind::NonStandard, false),
    ("2073", &["sport"], TitleKind::NonStandard, false),
    ("550", &["sport"], TitleKind::NonStandard, false),
    ("2124", &["sport"], TitleKind::NonStandard, false),
    ("1470", &["sport"], TitleKind::NonStandard, false),
    ("528", &["sport"], TitleKind::NonStandard, false),
    ("486", &["sport"], TitleKind::NonStandard, false),
    ("854", &["sport"], TitleKind::NonStandard, false),
    ("2079", &["sport"], TitleKind::NonStandard, false),
    ("1336", &["sport"], TitleKind::NonStandard, false),
    ("2171", &["sport"], TitleKind::NonStandard, false),
    ("1339", &["sport"], TitleKind::NonStandard, false),
    ("2455", &["sport"], TitleKind::NonStandard, false),
    ("1434", &["sport"], TitleKind::NonStandard, false),
    ("2350", &["sport"], TitleKind::NonStandard, false),
    ("1472", &["sport"], TitleKind::NonStandard, false),
    ("2068", &["sport"], TitleKind::NonStandard, false),
    ("2016", &["sport"], TitleKind::NonStandard, false),
];

/// Forum id → category metadata (insertion order preserved).
pub static MAP: Lazy<IndexMap<&'static str, Category>> = Lazy::new(|| {
    ROWS.iter()
        .map(|(id, types, kind, quick)| (*id, Category { types, title_kind: *kind, quick_parse: *quick }))
        .collect()
});

/// Every forum id in the map.
pub fn ids() -> Vec<&'static str> {
    MAP.keys().copied().collect()
}

/// Forum ids parsed by the hourly first-page cron.
pub fn quick_parse_ids() -> Vec<&'static str> {
    MAP.iter().filter(|(_, c)| c.quick_parse).map(|(k, _)| *k).collect()
}

pub fn get(id: &str) -> Option<&'static Category> {
    MAP.get(id)
}
