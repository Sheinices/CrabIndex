import { describe, expect, it, vi } from 'vitest'
import { createT, pluralTorrents } from './i18n.js'
import { clearRecentSearches, getRecentSearches, MAX_RECENT, pushRecentSearch, removeRecentSearch } from './recent.js'
import { barPercent, parseRuDate, sortTrackerStats, statsTotals } from './stats.js'
import { removeLegacyServiceWorkers } from './sw-cleanup.js'

describe('recent searches', () => {
  it('dedupes case-insensitively, most recent first, capped', () => {
    pushRecentSearch('Дюна')
    pushRecentSearch('матрица')
    pushRecentSearch('дюна')
    expect(getRecentSearches()).toEqual(['дюна', 'матрица'])
    for (let i = 0; i < 20; i += 1) pushRecentSearch(`q${i}`)
    expect(getRecentSearches()).toHaveLength(MAX_RECENT)
    expect(removeRecentSearch('q19')).not.toContain('q19')
    expect(clearRecentSearches()).toEqual([])
    expect(getRecentSearches()).toEqual([])
  })
  it('survives corrupted storage', () => {
    localStorage.setItem('crabindexRecentSearches', '{oops')
    expect(getRecentSearches()).toEqual([])
  })
})

describe('i18n', () => {
  it('interpolates and falls back', () => {
    expect(createT('ru')('search.found', { n: 5 })).toBe('Найдено: 5')
    expect(createT('en')('search.found', { n: 5 })).toBe('Found: 5')
    expect(createT('xx')('nav.search')).toBe('Поиск')
    expect(createT('en')('missing.key')).toBe('missing.key')
  })
  it('pluralises', () => {
    expect([1, 2, 5, 11, 21, 22].map((n) => pluralTorrents(n, 'ru'))).toEqual(['раздача', 'раздачи', 'раздач', 'раздач', 'раздача', 'раздачи'])
    expect(pluralTorrents(1, 'en')).toBe('torrent')
  })
})

describe('stats helpers', () => {
  const rows = [
    { trackerName: 'rutor', alltorrents: 983, newtor: 17, update: 983, lastnewtor: '25.09.2026' },
    { trackerName: 'kinozal', alltorrents: 5000, newtor: 0, update: 10, lastnewtor: '01.08.2026' },
  ]
  it('sorts and totals', () => {
    expect(sortTrackerStats(rows).map((r) => r.trackerName)).toEqual(['kinozal', 'rutor'])
    expect(sortTrackerStats(rows, 'name', 'asc').map((r) => r.trackerName)).toEqual(['kinozal', 'rutor'])
    expect(sortTrackerStats(rows, 'lastnewtor', 'desc')[0].trackerName).toBe('rutor')
    expect(statsTotals(rows)).toMatchObject({ trackers: 2, alltorrents: 5983, newtor: 17, active: 1, lastnewtor: '25.09.2026' })
  })
  it('parses dates and computes bars', () => {
    expect(parseRuDate('25.09.2026')).toBe(Date.UTC(2026, 8, 25))
    expect(parseRuDate('')).toBe(0)
    expect(barPercent(50, 100)).toBe(50)
    expect(barPercent(1, 100000)).toBe(1.5)
    expect(barPercent(0, 100)).toBe(0)
  })
})

describe('service worker cleanup', () => {
  it('unregisters every worker and clears caches', async () => {
    const unregister = vi.fn(async () => true)
    const nav = { serviceWorker: { getRegistrations: async () => [{ unregister }, { unregister }] } }
    const del = vi.fn(async () => true)
    await removeLegacyServiceWorkers(nav, { keys: async () => ['a', 'b'], delete: del })
    expect(unregister).toHaveBeenCalledTimes(2)
    expect(del).toHaveBeenCalledTimes(2)
  })
  it('is a no-op without support', async () => {
    await expect(removeLegacyServiceWorkers({}, undefined)).resolves.toBeUndefined()
  })
})
