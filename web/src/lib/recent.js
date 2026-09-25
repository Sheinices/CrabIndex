import { KEYS, readItem, writeItem } from './storage.js'

export const MAX_RECENT = 8

export function getRecentSearches() {
  try {
    const parsed = JSON.parse(readItem(KEYS.recent) || '[]')
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((x) => typeof x === 'string' && x.trim())
      .map((x) => x.trim())
      .slice(0, MAX_RECENT)
  } catch {
    return []
  }
}

export function pushRecentSearch(query) {
  const q = String(query || '').trim()
  if (!q) return getRecentSearches()
  const next = [q, ...getRecentSearches().filter((x) => x.toLowerCase() !== q.toLowerCase())].slice(0, MAX_RECENT)
  writeItem(KEYS.recent, JSON.stringify(next))
  return next
}

export function removeRecentSearch(query) {
  const next = getRecentSearches().filter((x) => x !== query)
  writeItem(KEYS.recent, JSON.stringify(next))
  return next
}

export function clearRecentSearches() {
  writeItem(KEYS.recent, '')
  return []
}
