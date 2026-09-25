/** Helpers for the /stats page (data from GET /stats/torrents). */

export const STAT_COLUMNS = ['alltorrents', 'newtor', 'update', 'lastnewtor']

/** "25.09.2026" → timestamp (0 when unknown). */
export function parseRuDate(value) {
  const m = /^(\d{1,2})\.(\d{1,2})\.(\d{4})/.exec(String(value || '').trim())
  if (!m) return 0
  return Date.UTC(Number(m[3]), Number(m[2]) - 1, Number(m[1]))
}

function sortValue(row, key) {
  if (key === 'name') return String(row.trackerName || '').toLowerCase()
  if (key === 'lastnewtor') return parseRuDate(row.lastnewtor)
  return Number(row[key]) || 0
}

export function sortTrackerStats(rows, key = 'alltorrents', dir = 'desc') {
  const sign = dir === 'asc' ? 1 : -1
  return [...rows].sort((a, b) => {
    const va = sortValue(a, key)
    const vb = sortValue(b, key)
    const cmp = typeof va === 'string' ? va.localeCompare(vb) : va - vb
    return cmp * sign || String(a.trackerName).localeCompare(String(b.trackerName))
  })
}

export function statsTotals(rows) {
  const totals = { trackers: rows.length, alltorrents: 0, newtor: 0, update: 0, active: 0, lastnewtor: '' }
  let latest = 0
  for (const r of rows) {
    totals.alltorrents += Number(r.alltorrents) || 0
    totals.newtor += Number(r.newtor) || 0
    totals.update += Number(r.update) || 0
    if ((Number(r.newtor) || 0) > 0) totals.active += 1
    const t = parseRuDate(r.lastnewtor)
    if (t > latest) {
      latest = t
      totals.lastnewtor = r.lastnewtor
    }
  }
  return totals
}

/** Share of `value` against `max`, clamped to 0-100 with a visible minimum for non-zero values. */
export function barPercent(value, max) {
  const v = Number(value) || 0
  const m = Number(max) || 0
  if (v <= 0 || m <= 0) return 0
  return Math.max(1.5, Math.min(100, (v / m) * 100))
}

export function formatNumber(value, locale = 'ru') {
  return new Intl.NumberFormat(locale === 'ru' ? 'ru-RU' : 'en-US').format(Number(value) || 0)
}
