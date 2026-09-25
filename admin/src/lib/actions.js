/**
 * Cron actions supported per tracker (`{base}/api/cron/{slug}/{action}`).
 * Built from the tracker routers in `crates/crab-trackers-*` - keep in sync.
 */

const PAGE = { name: 'page', label: 'Страница', type: 'number', placeholder: '0' }
const PAGES = (def) => ({ name: 'pages', label: 'Страниц', type: 'number', placeholder: String(def) })
const LIMIT_PAGE = { name: 'limit_page', label: 'Страниц на раздел', type: 'number', placeholder: '0 - все' }
const RANGE = [
  { name: 'parsefrom', label: 'С страницы', type: 'number', placeholder: '0' },
  { name: 'parseto', label: 'По страницу', type: 'number', placeholder: '0' },
]

export const ACTIONS = {
  parse: { label: 'Парсинг', description: 'Обычный проход по свежим страницам', params: [] },
  updatetasksparse: {
    label: 'Обновить задачи',
    description: 'Пересобрать карту страниц для ParseAll (фоново)',
    params: [],
  },
  parsealltask: {
    label: 'ParseAll',
    description: 'Полный проход по всем страницам (многочасовой, фоново)',
    params: [],
    heavy: true,
  },
  parselatest: {
    label: 'Последние страницы',
    description: 'Первые N страниц каждой категории',
    params: [PAGES(5)],
  },
  backfill: { label: 'Backfill', description: 'Догрузка архива', params: [PAGES(20)], heavy: true },
  backfillstatus: { label: 'Статус backfill', description: 'Состояние догрузки архива', params: [], readOnly: true },
  parsefromdate: {
    label: 'С даты',
    description: 'Архив старше указанной даты',
    params: [
      { name: 'lastnewtor', label: 'Дата (lastnewtor)', type: 'text', placeholder: '2024-01-31' },
      PAGES(20),
    ],
    heavy: true,
  },
  parsepages: {
    label: 'Страницы',
    description: 'Диапазон страниц /new/',
    params: [
      { name: 'pagefrom', label: 'С', type: 'number', placeholder: '1' },
      { name: 'pageto', label: 'По', type: 'number', placeholder: '1' },
    ],
  },
  parseseasonpacks: {
    label: 'Сезонные паки',
    description: 'Паки сезонов (все или одного сериала)',
    params: [{ name: 'series', label: 'Сериал (slug)', type: 'text', placeholder: 'необязательно' }],
    heavy: true,
  },
  verifypage: {
    label: 'Проверить страницу',
    description: 'Диагностика страницы сериала',
    params: [{ name: 'series', label: 'Сериал (slug)', type: 'text', placeholder: 'The_Bear' }],
    readOnly: true,
  },
  stats: { label: 'Статистика', description: 'Статистика парсера', params: [], readOnly: true },
  parseshows: { label: 'Шоу', description: 'Полный проход по списку шоу', params: [], heavy: true },
  parseshowstatus: { label: 'Статус шоу', description: 'Прогресс прохода по шоу', params: [], readOnly: true },
  takelogin: { label: 'Авторизация', description: 'Выполнить вход на трекер', params: [] },
}

const FULL = ['parse', 'updatetasksparse', 'parsealltask', 'parselatest']
const PARSE_ONLY = ['parse']

export const TRACKER_ACTIONS = {
  anibelka: FULL,
  anidub: PARSE_ONLY,
  anifilm: PARSE_ONLY,
  aniliberty: PARSE_ONLY,
  animelayer: ['parse', 'takelogin'],
  anistar: PARSE_ONLY,
  baibako: PARSE_ONLY,
  bitru: ['parse', 'backfill', 'parsefromdate'],
  kinozal: FULL,
  knaben: ['parse', 'backfill', 'backfillstatus'],
  korsars: FULL,
  leproduction: PARSE_ONLY,
  lostfilm: ['parse', 'parsepages', 'parseseasonpacks', 'verifypage', 'stats'],
  mazepa: PARSE_ONLY,
  megapeer: FULL,
  nnmclub: FULL,
  rudub: PARSE_ONLY,
  rutor: FULL,
  rutracker: FULL,
  selezen: PARSE_ONLY,
  subsplease: ['parse', 'parseshows', 'parseshowstatus'],
  toloka: FULL,
  torrentby: FULL,
  ultradox: FULL,
  viruseproject: PARSE_ONLY,
}

/** Per-tracker parameter overrides (e.g. parse takes a page range). */
const PARAM_OVERRIDES = {
  'animelayer:parse': RANGE,
  'selezen:parse': RANGE,
  'aniliberty:parse': RANGE,
  'anidub:parse': RANGE,
  'baibako:parse': RANGE,
  'rudub:parse': RANGE,
  'anistar:parse': [LIMIT_PAGE],
  'leproduction:parse': [LIMIT_PAGE],
  'viruseproject:parse': [LIMIT_PAGE],
  'anifilm:parse': [{ name: 'fullparse', label: 'Полный проход (true/false)', type: 'text', placeholder: 'false' }],
  'knaben:parse': [PAGES(1)],
  'knaben:backfill': [PAGES(10)],
  'subsplease:parse': [PAGES(1)],
  'rutracker:parsealltask': [
    { name: 'cat', label: 'Раздел (cat)', type: 'text', placeholder: 'все' },
    { name: 'maxpages', label: 'Макс. страниц', type: 'number', placeholder: '0 - все' },
  ],
}

const PAGE_PARSE = new Set(['anibelka', 'kinozal', 'korsars', 'megapeer', 'nnmclub', 'rutor', 'rutracker', 'toloka', 'torrentby', 'ultradox'])

/** Actions a tracker supports, as `[{ id, label, description, params, heavy, readOnly }]`. */
export function actionsFor(slug) {
  const ids = TRACKER_ACTIONS[String(slug || '').toLowerCase()] || []
  return ids.map((id) => {
    const def = ACTIONS[id]
    const key = `${slug}:${id}`
    let params = PARAM_OVERRIDES[key] || def.params
    if (id === 'parse' && PAGE_PARSE.has(slug) && !PARAM_OVERRIDES[key]) params = [PAGE]
    return { id, ...def, params }
  })
}

export function supportsAction(slug, action) {
  return (TRACKER_ACTIONS[String(slug || '').toLowerCase()] || []).includes(action)
}

/** Only ParseAll-capable trackers show the ParseAll column. */
export function hasParseAll(slug) {
  return supportsAction(slug, 'parsealltask')
}

/** Drop empty param values so server defaults apply. */
export function cleanParams(values) {
  const out = {}
  for (const [k, v] of Object.entries(values || {})) {
    const s = String(v ?? '').trim()
    if (s) out[k] = s
  }
  return out
}

export function trackerLogName(slug) {
  return `${String(slug).toLowerCase()}.log`
}
