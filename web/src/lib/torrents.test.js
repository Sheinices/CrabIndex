import { describe, expect, it } from 'vitest'
import { ITEMS, NOW } from '../test/fixtures.js'
import {
  buildFacets,
  countActiveFilters,
  EMPTY_FILTERS,
  filterItems,
  filtersFromParams,
  formatBytes,
  formatSeasons,
  parseTextFilter,
  qualityLabel,
  queryKind,
  sortFromParams,
  sortItems,
  stateToParams,
  trackerIcon,
} from './torrents.js'

const f = (over) => ({ ...EMPTY_FILTERS, ...over })
const titles = (list) => list.map((i) => i.tracker + ':' + i.quality)

describe('filterItems', () => {
  it('returns everything without filters', () => {
    expect(filterItems(ITEMS, EMPTY_FILTERS, { now: NOW })).toBe(ITEMS)
  })

  it('ORs values inside a facet and ANDs facets', () => {
    expect(titles(filterItems(ITEMS, f({ quality: ['2160', '1080'] }), { now: NOW }))).toEqual(['kinozal:2160', 'rutor:1080'])
    expect(titles(filterItems(ITEMS, f({ quality: ['2160', '1080'], tracker: ['rutor'] }), { now: NOW }))).toEqual(['rutor:1080'])
  })

  it('filters by video type, voice, season, type and year', () => {
    expect(filterItems(ITEMS, f({ video: ['hdr'] }), { now: NOW })).toHaveLength(1)
    expect(filterItems(ITEMS, f({ voice: ['LostFilm'] }), { now: NOW })).toHaveLength(2)
    expect(filterItems(ITEMS, f({ season: ['2'] }), { now: NOW })).toHaveLength(1)
    expect(filterItems(ITEMS, f({ type: ['documovie'] }), { now: NOW })).toHaveLength(1)
    expect(filterItems(ITEMS, f({ year: ['1999'] }), { now: NOW })).toHaveLength(1)
  })

  it('filters by added period', () => {
    expect(filterItems(ITEMS, f({ period: 'day' }), { now: NOW })).toHaveLength(1)
    expect(filterItems(ITEMS, f({ period: 'month' }), { now: NOW })).toHaveLength(1)
    expect(filterItems(ITEMS, f({ period: 'year' }), { now: NOW })).toHaveLength(2)
  })

  it('refines by words and excludes "-words" (ё-insensitive)', () => {
    expect(filterItems(ITEMS, f({ text: 'dinosaurs' }), { now: NOW })).toHaveLength(2)
    expect(filterItems(ITEMS, f({ text: 'dinosaurs -hdr' }), { now: NOW })).toHaveLength(1)
    expect(parseTextFilter('Ёлки -2160p -')).toEqual({ include: ['елки'], exclude: ['2160p'] })
  })
})

describe('buildFacets', () => {
  it('counts values and sorts them sensibly', () => {
    const facets = buildFacets(ITEMS, EMPTY_FILTERS, { now: NOW })
    expect(facets.quality.map((o) => o.value)).toEqual(['2160', '1080', '480'])
    expect(facets.tracker).toEqual([
      { value: 'rutor', count: 2 },
      { value: 'kinozal', count: 1 },
    ])
    expect(facets.season).toEqual([
      { value: '1', count: 2 },
      { value: '2', count: 1 },
    ])
  })

  it('counts a facet against the other active filters only', () => {
    const facets = buildFacets(ITEMS, f({ tracker: ['kinozal'] }), { now: NOW })
    // tracker counts ignore the tracker filter itself
    expect(facets.tracker.find((o) => o.value === 'rutor').count).toBe(2)
    // other facets are narrowed by it
    expect(facets.quality).toEqual([{ value: '2160', count: 1 }])
  })

  it('keeps selected values that no longer match', () => {
    const facets = buildFacets(ITEMS, f({ tracker: ['kinozal'], quality: ['720'] }), { now: NOW })
    expect(facets.quality).toContainEqual({ value: '720', count: 0 })
  })
})

describe('sortItems', () => {
  it('sorts by seeders desc by default', () => {
    expect(sortItems(ITEMS).map((i) => i.sid)).toEqual([40, 12, 5])
  })
  it('sorts by size, date, peers in both directions', () => {
    expect(sortItems(ITEMS, 'size', 'asc').map((i) => i.quality)).toEqual([480, 1080, 2160])
    expect(sortItems(ITEMS, 'date', 'desc').map((i) => i.quality)).toEqual([480, 2160, 1080])
    expect(sortItems(ITEMS, 'pir', 'desc').map((i) => i.pir)).toEqual([9, 3, 0])
  })
  it('does not mutate the input', () => {
    const copy = [...ITEMS]
    sortItems(ITEMS, 'size', 'asc')
    expect(ITEMS).toEqual(copy)
  })
})

describe('URL state', () => {
  it('round-trips filters and sort', () => {
    const filters = f({ tracker: ['rutor', 'kinozal'], quality: ['2160'], period: 'week', text: 'hdr' })
    const params = stateToParams({ q: 'Дюна', filters, sort: { key: 'size', dir: 'asc' } })
    expect(params.get('q')).toBe('Дюна')
    expect(params.get('tracker')).toBe('rutor,kinozal')
    expect(filtersFromParams(params)).toEqual(filters)
    expect(sortFromParams(params)).toEqual({ key: 'size', dir: 'asc' })
  })
  it('omits defaults and ignores junk', () => {
    expect(stateToParams({ q: 'x', filters: EMPTY_FILTERS, sort: { key: 'sid', dir: 'desc' } }).toString()).toBe('q=x')
    const p = new URLSearchParams('sort=evil&dir=up&period=decade')
    expect(sortFromParams(p)).toEqual({ key: 'sid', dir: 'desc' })
    expect(filtersFromParams(p).period).toBe('')
  })
  it('counts active filters', () => {
    expect(countActiveFilters(f({ tracker: ['a', 'b'], period: 'day', text: ' x ' }))).toBe(4)
  })
})

describe('formatting', () => {
  it('formats quality, seasons, bytes', () => {
    expect(qualityLabel(2160)).toBe('4K')
    expect(qualityLabel(1080)).toBe('1080p')
    expect(qualityLabel(0)).toBe('')
    expect(formatSeasons([3, 1, 2, 5])).toBe('1-3, 5')
    expect(formatSeasons([1, 2])).toBe('1, 2')
    expect(formatBytes(3382286745, 'en')).toBe('3.15 GB')
    expect(formatBytes(0)).toBe('')
  })
  it('builds safe tracker icon paths', () => {
    expect(trackerIcon('rutor')).toBe('/img/ico/rutor.ico')
    expect(trackerIcon('../etc')).toBe('/img/ico/etc.ico')
    expect(trackerIcon('')).toBe('/img/ico/default.ico')
  })
  it('detects id queries', () => {
    expect(queryKind('tt0133093')).toBe('imdb')
    expect(queryKind('kp301')).toBe('kp')
    expect(queryKind('Матрица')).toBe('text')
  })
})
