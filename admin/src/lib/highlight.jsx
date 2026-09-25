/** Split `text` into plain strings and <mark> elements for `query` (case-insensitive). */
export function highlight(text, query) {
  const q = String(query || '')
  if (!q) return text
  const lower = text.toLowerCase()
  const needle = q.toLowerCase()
  const out = []
  let i = 0
  let k = 0
  for (;;) {
    const j = lower.indexOf(needle, i)
    if (j < 0) break
    if (j > i) out.push(text.slice(i, j))
    out.push(<mark key={k++}>{text.slice(j, j + needle.length)}</mark>)
    i = j + needle.length
  }
  if (i < text.length) out.push(text.slice(i))
  return out
}

export function levelClass(line) {
  if (/\b(ERROR|ERR|FATAL|CRIT(ICAL)?|PANIC)\b/.test(line)) return 'text-danger'
  if (/\b(WARN(ING)?)\b/.test(line)) return 'text-warn'
  if (/\b(DEBUG|TRACE)\b/.test(line)) return 'text-muted'
  return ''
}
