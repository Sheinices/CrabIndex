/**
 * Pure helpers for search results: facets, client-side filters, sorting, formatting.
 * Items are the raw objects returned by GET /api/v1.0/torrents.
 */

/** Multi-value facets, in the order they are shown in the filter panel. */
export const FACETS = ['type', 'quality', 'video', 'tracker', 'voice', 'season', 'year']

export const PERIODS = ['day', 'week', 'month', 'year']

const PERIOD_MS = {
  day: 86_400_000,
  week: 7 * 86_400_000,
  month: 31 * 86_400_000,
  year: 366 * 86_400_000,
}

export const SORT_KEYS = ['sid', 'pir', 'size', 'date', 'update', 'year']
export const DEFAULT_SORT = { key: 'sid', dir: 'desc' }

export const TYPE_ORDER = ['movie', 'serial', 'multfilm', 'multserial', 'anime', 'documovie', 'docuserial', 'tvshow', 'sport']

export const EMPTY_FILTERS = Object.freeze({
  type: [],
  quality: [],
  video: [],
  tracker: [],
  voice: [],
  season: [],
  year: [],
  period: '',
  text: '',
})

/** Values of `facet` carried by one item, as strings. */
export function facetValues(item, facet) {
  switch (facet) {
    case 'tracker':
      return item.tracker ? [String(item.tracker).toLowerCase()] : []
    case 'quality': {
      const q = Number(item.quality)
      return q > 0 ? [String(q)] : []
    }
    case 'video':
      return item.videotype ? [String(item.videotype).toLowerCase()] : []
    case 'voice':
      return Array.isArray(item.voices) ? item.voices.filter(Boolean).map(String) : []
    case 'season':
      return Array.isArray(item.seasons) ? item.seasons.filter((s) => s !== null && s !== '').map(String) : []
    case 'type':
      return Array.isArray(item.types) ? item.types.filter(Boolean).map(String) : []
    case 'year': {
      const y = Number(item.relased)
      return y > 0 ? [String(y)] : []
    }
    default:
      return []
  }
}

export function toTime(value) {
  if (value === null || value === undefined || value === '') return 0
  const d = typeof value === 'number' ? new Date(value < 1e12 ? value * 1000 : value) : new Date(value)
  const t = d.getTime()
  return Number.isNaN(t) ? 0 : t
}

/** Search key: lowercase, `ё` → `е`. */
export function normalizeText(value) {
  return String(value || '')
    .toLowerCase()
    .replace(/ё/g, 'е')
}

/** Parses "word -excluded" into include/exclude token lists. */
export function parseTextFilter(text) {
  const include = []
  const exclude = []
  for (const token of normalizeText(text).split(/\s+/)) {
    if (!token) continue
    if (token.startsWith('-') && token.length > 1) exclude.push(token.slice(1))
    else if (token !== '-') include.push(token)
  }
  return { include, exclude }
}

function itemText(item) {
  return normalizeText([item.title, item.name, item.originalname].filter(Boolean).join(' '))
}

/**
 * Returns items matching every active filter.
 * `except` skips one facet (used to compute counts for that facet).
 */
export function filterItems(items, filters, { now = Date.now(), except } = {}) {
  const f = { ...EMPTY_FILTERS, ...filters }
  const activeFacets = FACETS.filter((k) => k !== except && f[k]?.length)
  const text = parseTextFilter(f.text)
  const periodMs = PERIOD_MS[f.period]

  if (!activeFacets.length && !text.include.length && !text.exclude.length && !periodMs) return items

  return items.filter((item) => {
    for (const facet of activeFacets) {
      const values = facetValues(item, facet)
      if (!values.some((v) => f[facet].includes(v))) return false
    }
    if (periodMs) {
      const t = toTime(item.createTime)
      if (!t || now - t > periodMs) return false
    }
    if (text.include.length || text.exclude.length) {
      const hay = itemText(item)
      if (text.include.some((w) => !hay.includes(w))) return false
      if (text.exclude.some((w) => hay.includes(w))) return false
    }
    return true
  })
}

function compareFacetValues(facet, a, b) {
  switch (facet) {
    case 'quality':
    case 'year':
      return Number(b.value) - Number(a.value)
    case 'season':
      return Number(a.value) - Number(b.value)
    case 'type': {
      const ia = TYPE_ORDER.indexOf(a.value)
      const ib = TYPE_ORDER.indexOf(b.value)
      return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.value.localeCompare(b.value)
    }
    case 'video':
      return a.value.localeCompare(b.value)
    default:
      return b.count - a.count || a.value.localeCompare(b.value, 'ru')
  }
}

/**
 * Facet options with counts. Counts for a facet respect every *other* active filter,
 * so the user sees how many results a click would give. Selected values are always kept.
 */
export function buildFacets(items, filters, { now = Date.now() } = {}) {
  const f = { ...EMPTY_FILTERS, ...filters }
  const out = {}
  for (const facet of FACETS) {
    const pool = filterItems(items, f, { now, except: facet })
    const counts = new Map()
    for (const item of pool) {
      for (const v of new Set(facetValues(item, facet))) counts.set(v, (counts.get(v) || 0) + 1)
    }
    for (const v of f[facet]) if (!counts.has(v)) counts.set(v, 0)
    out[facet] = [...counts].map(([value, count]) => ({ value, count })).sort((a, b) => compareFacetValues(facet, a, b))
  }
  return out
}

const SORT_GETTERS = {
  sid: (i) => Number(i.sid) || 0,
  pir: (i) => Number(i.pir) || 0,
  size: (i) => Number(i.size) || 0,
  date: (i) => toTime(i.createTime),
  update: (i) => toTime(i.updateTime),
  year: (i) => Number(i.relased) || 0,
}

/** Stable sort; ties are broken by seeders, then by date. */
export function sortItems(items, key = DEFAULT_SORT.key, dir = DEFAULT_SORT.dir) {
  const get = SORT_GETTERS[key] || SORT_GETTERS.sid
  const sign = dir === 'asc' ? 1 : -1
  return items
    .map((item, index) => ({ item, index }))
    .sort(
      (a, b) =>
        (get(a.item) - get(b.item)) * sign ||
        SORT_GETTERS.sid(b.item) - SORT_GETTERS.sid(a.item) ||
        SORT_GETTERS.date(b.item) - SORT_GETTERS.date(a.item) ||
        a.index - b.index,
    )
    .map((x) => x.item)
}

export function countActiveFilters(filters) {
  const f = { ...EMPTY_FILTERS, ...filters }
  let n = FACETS.reduce((sum, k) => sum + (f[k]?.length || 0), 0)
  if (f.period) n += 1
  if (f.text.trim()) n += 1
  return n
}

/* ---------- URL <-> state ---------- */

const URL_FACET_KEYS = { type: 'type', quality: 'quality', video: 'video', tracker: 'tracker', voice: 'voice', season: 'season', year: 'year' }

function splitList(value) {
  if (!value) return []
  return [...new Set(value.split(',').map((s) => s.trim()).filter(Boolean))]
}

export function filtersFromParams(params) {
  const f = { ...EMPTY_FILTERS }
  for (const [facet, key] of Object.entries(URL_FACET_KEYS)) f[facet] = splitList(params.get(key))
  const period = params.get('period') || ''
  f.period = PERIODS.includes(period) ? period : ''
  f.text = params.get('in') || ''
  return f
}

export function sortFromParams(params) {
  const key = params.get('sort')
  const dir = params.get('dir')
  return {
    key: SORT_KEYS.includes(key) ? key : DEFAULT_SORT.key,
    dir: dir === 'asc' || dir === 'desc' ? dir : DEFAULT_SORT.dir,
  }
}

/** Writes query, filters and sort into a new URLSearchParams (defaults omitted). */
export function stateToParams({ q, filters, sort }) {
  const p = new URLSearchParams()
  if (q) p.set('q', q)
  const f = { ...EMPTY_FILTERS, ...filters }
  for (const [facet, key] of Object.entries(URL_FACET_KEYS)) if (f[facet].length) p.set(key, f[facet].join(','))
  if (f.period) p.set('period', f.period)
  if (f.text) p.set('in', f.text)
  if (sort && sort.key !== DEFAULT_SORT.key) p.set('sort', sort.key)
  if (sort && sort.dir !== DEFAULT_SORT.dir) p.set('dir', sort.dir)
  return p
}

/* ---------- formatting ---------- */

export function qualityLabel(q) {
  const n = Number(q)
  if (!n || n < 1) return ''
  if (n >= 4320) return '8K'
  if (n >= 2160) return '4K'
  return `${n}p`
}

const SIZE_UNITS = {
  ru: ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ'],
  en: ['B', 'KB', 'MB', 'GB', 'TB'],
}

export function formatBytes(bytes, locale = 'ru') {
  const n = Number(bytes)
  if (!n || n < 0) return ''
  const units = SIZE_UNITS[locale] || SIZE_UNITS.en
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  const digits = v >= 100 || i === 0 ? 0 : v >= 10 ? 1 : 2
  return `${new Intl.NumberFormat(locale === 'ru' ? 'ru-RU' : 'en-US', { maximumFractionDigits: digits }).format(v)} ${units[i]}`
}

export function formatSize(item, locale) {
  return formatBytes(item.size, locale) || item.sizeName || ''
}

export function formatDate(value, locale = 'ru') {
  const t = toTime(value)
  if (!t) return ''
  return new Intl.DateTimeFormat(locale === 'ru' ? 'ru-RU' : 'en-GB', { day: 'numeric', month: 'short', year: 'numeric' }).format(t)
}

export function formatDateTime(value, locale = 'ru') {
  const t = toTime(value)
  if (!t) return ''
  return new Intl.DateTimeFormat(locale === 'ru' ? 'ru-RU' : 'en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(t)
}

/** [1,2,3,5] → "1-3, 5" */
export function formatSeasons(seasons) {
  const nums = [...new Set((seasons || []).map(Number).filter((n) => Number.isFinite(n) && n >= 0))].sort((a, b) => a - b)
  const parts = []
  for (let i = 0; i < nums.length; i += 1) {
    let j = i
    while (j + 1 < nums.length && nums[j + 1] === nums[j] + 1) j += 1
    parts.push(j > i + 1 ? `${nums[i]}-${nums[j]}` : j === i + 1 ? `${nums[i]}, ${nums[j]}` : `${nums[i]}`)
    i = j
  }
  return parts.join(', ')
}

export const TRACKER_LABELS = {
  anibelka: 'Anibelka',
  anidub: 'AniDub',
  anifilm: 'AniFilm',
  aniliberty: 'AniLiberty',
  anilibria: 'AniLibria',
  animelayer: 'AnimeLayer',
  anistar: 'AniStar',
  baibako: 'Baibako',
  bitru: 'BitRu',
  hdrezka: 'HDRezka',
  kinozal: 'Kinozal',
  knaben: 'Knaben',
  korsars: 'Korsars',
  leproduction: 'LE-Production',
  lostfilm: 'LostFilm',
  mazepa: 'Mazepa',
  megapeer: 'MegaPeer',
  nnmclub: 'NNM-Club',
  rudub: 'RuDub',
  rutor: 'Rutor',
  rutracker: 'RuTracker',
  selezen: 'Selezen',
  subsplease: 'SubsPlease',
  toloka: 'Toloka',
  torrentby: 'Torrent.by',
  ultradox: 'UltraDox',
  underverse: 'Underverse',
  viruseproject: 'ViruseProject',
}

export function trackerLabel(slug) {
  const key = String(slug || '').toLowerCase()
  if (!key) return '-'
  return TRACKER_LABELS[key] || key.charAt(0).toUpperCase() + key.slice(1)
}

export function trackerIcon(slug) {
  const safe = String(slug || '')
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '')
  return `/img/ico/${safe || 'default'}.ico`
}

export function torrentKey(item, index) {
  return item.magnet || item.url || `${item.tracker}|${item.title}|${index}`
}

/** Recognises IMDb / Kinopoisk / TMDB ids so the UI can hint what is being searched. */
export function queryKind(q) {
  const s = String(q || '').trim().toLowerCase()
  if (/^tt\d{5,}$/.test(s)) return 'imdb'
  if (/^kp\d+$/.test(s)) return 'kp'
  if (/^tmdb\d+$/.test(s) || s.includes('themoviedb.org')) return 'tmdb'
  return 'text'
}
