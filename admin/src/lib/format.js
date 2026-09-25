const nf = new Intl.NumberFormat('ru-RU')

export function formatNumber(n) {
  const v = Number(n)
  return Number.isFinite(v) ? nf.format(v) : '-'
}

export function formatBytes(n) {
  const v = Number(n)
  if (!Number.isFinite(v) || v < 0) return '-'
  const units = ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ']
  let i = 0
  let x = v
  while (x >= 1024 && i < units.length - 1) {
    x /= 1024
    i++
  }
  return `${i === 0 ? x : x.toFixed(x < 10 ? 1 : 0)} ${units[i]}`
}

export function formatDuration(seconds) {
  const s = Math.floor(Number(seconds))
  if (!Number.isFinite(s) || s < 0) return '-'
  const d = Math.floor(s / 86400)
  const h = Math.floor((s % 86400) / 3600)
  const m = Math.floor((s % 3600) / 60)
  if (d) return `${d} д ${h} ч`
  if (h) return `${h} ч ${m} мин`
  if (m) return `${m} мин`
  return `${s} с`
}

export function formatDate(value) {
  if (!value) return '-'
  const d = new Date(value)
  if (Number.isNaN(d.getTime())) return String(value)
  return d.toLocaleString('ru-RU')
}

export function formatRelative(value, now = Date.now()) {
  if (!value) return '-'
  const t = new Date(value).getTime()
  if (Number.isNaN(t)) return String(value)
  const diff = Math.round((now - t) / 1000)
  if (diff < 0) return formatDate(value)
  if (diff < 60) return 'только что'
  return `${formatDuration(diff)} назад`
}

/** Pretty-print a cron/dev response (JSON or text). */
export function prettyResponse(data) {
  if (data == null) return ''
  if (typeof data === 'string') {
    const t = data.trim()
    if (/^[[{]/.test(t)) {
      try {
        return JSON.stringify(JSON.parse(t), null, 2)
      } catch {
        return data
      }
    }
    return data
  }
  return JSON.stringify(data, null, 2)
}

export function jobPercent(job) {
  if (typeof job?.percent === 'number' && Number.isFinite(job.percent)) return Math.max(0, Math.min(100, job.percent))
  const done = Number(job?.pagesCompleted)
  const total = Number(job?.pagesTotal)
  if (total > 0 && Number.isFinite(done)) return Math.round((done / total) * 100)
  return null
}
