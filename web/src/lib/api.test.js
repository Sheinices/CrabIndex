import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, buildUrl, fetchJson, getConf, searchTorrents } from './api.js'
import { getApiKey, setApiKey } from './apikey.js'

function mockFetch(body, status = 200) {
  const fn = vi.fn(async () => ({ ok: status >= 200 && status < 300, status, json: async () => body }))
  vi.stubGlobal('fetch', fn)
  return fn
}

afterEach(() => vi.unstubAllGlobals())

describe('buildUrl', () => {
  it('skips empty params and encodes values', () => {
    expect(buildUrl('/x', { a: '', b: null, c: undefined, d: false, e: 'Дюна 2', f: ['a', '', 'b'] })).toBe('/x?e=%D0%94%D1%8E%D0%BD%D0%B0+2&f=a%2Cb')
    expect(buildUrl('/x')).toBe('/x')
  })
})

describe('searchTorrents', () => {
  it('passes the query and apikey', async () => {
    const fn = mockFetch([{ title: 'a' }])
    const items = await searchTorrents('  Динозавры ', 'secret')
    expect(items).toEqual([{ title: 'a' }])
    const url = new URL(fn.mock.calls[0][0], 'http://x')
    expect(url.pathname).toBe('/api/v1.0/torrents')
    expect(url.searchParams.get('search')).toBe('Динозавры')
    expect(url.searchParams.get('apikey')).toBe('secret')
  })

  it('omits apikey when none is set', async () => {
    const fn = mockFetch([])
    await searchTorrents('Дюна', '')
    expect(fn.mock.calls[0][0]).not.toContain('apikey')
  })

  it('does not call the server for queries shorter than 2 chars', async () => {
    const fn = mockFetch([])
    expect(await searchTorrents('a', '')).toEqual([])
    expect(fn).not.toHaveBeenCalled()
  })

  it('treats non-array responses as empty', async () => {
    mockFetch({ error: 'x' })
    expect(await searchTorrents('Дюна', '')).toEqual([])
  })
})

describe('errors', () => {
  it('throws ApiError with status; 401 is unauthorized', async () => {
    mockFetch({}, 401)
    const err = await fetchJson('/api/v1.0/torrents').catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(401)
    expect(err.unauthorized).toBe(true)
  })

  it('maps network failures to status 0', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('offline'))))
    const err = await fetchJson('/x').catch((e) => e)
    expect(err.status).toBe(0)
    expect(err.unauthorized).toBe(false)
  })
})

describe('conf + apikey storage', () => {
  it('checks the stored key against /api/v1.0/conf', async () => {
    setApiKey('  k1  ')
    expect(getApiKey()).toBe('k1')
    const fn = mockFetch({ configured: true, apikey: true })
    await getConf(getApiKey())
    expect(fn.mock.calls[0][0]).toBe('/api/v1.0/conf?apikey=k1')
    setApiKey('')
    expect(localStorage.getItem('api_key')).toBeNull()
  })
})
