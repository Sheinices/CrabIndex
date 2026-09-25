import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { api, apiUrl, ApiError, getConfig, login, onUnauthorized, runCron, saveConfig } from './api.js'
import { setBaseForTests } from './base.js'

function mockFetch(status, body, contentType = 'application/json') {
  const text = typeof body === 'string' ? body : JSON.stringify(body)
  const fn = vi.fn(async () => new Response(text, { status, headers: { 'content-type': contentType } }))
  vi.stubGlobal('fetch', fn)
  return fn
}

describe('api client', () => {
  beforeEach(() => setBaseForTests('/secret-panel'))
  afterEach(() => vi.unstubAllGlobals())

  it('builds URLs under {base}/api and drops empty query values', () => {
    expect(apiUrl('overview')).toBe('/secret-panel/api/overview')
    expect(apiUrl('/logs/app.log', { lines: 300, x: '', y: null })).toBe('/secret-panel/api/logs/app.log?lines=300')
  })

  it('sends X-Crab-Admin and same-origin credentials on GET', async () => {
    const fetch = mockFetch(200, { ok: true })
    await getConfig('yaml')
    const [url, init] = fetch.mock.calls[0]
    expect(url).toBe('/secret-panel/api/config?format=yaml')
    expect(init.credentials).toBe('same-origin')
    expect(init.headers['X-Crab-Admin']).toBe('1')
    expect(init.method).toBe('GET')
  })

  it('sends JSON body with the header on POST', async () => {
    const fetch = mockFetch(200, { ok: true })
    await saveConfig({ data: { a: 1 }, format: 'yaml' })
    const [url, init] = fetch.mock.calls[0]
    expect(url).toBe('/secret-panel/api/config')
    expect(init.method).toBe('POST')
    expect(init.headers['X-Crab-Admin']).toBe('1')
    expect(init.headers['Content-Type']).toBe('application/json')
    expect(JSON.parse(init.body)).toEqual({ data: { a: 1 }, format: 'yaml' })
  })

  it('notifies unauthorized listeners on 401 and throws ApiError', async () => {
    mockFetch(401, { error: 'unauthorized' })
    const spy = vi.fn()
    const off = onUnauthorized(spy)
    await expect(api('overview')).rejects.toMatchObject({ status: 401, message: 'unauthorized' })
    expect(spy).toHaveBeenCalledTimes(1)
    off()
  })

  it('does not trigger the unauthorized flow for a wrong login key', async () => {
    mockFetch(401, { ok: false, error: 'invalid key' })
    const spy = vi.fn()
    const off = onUnauthorized(spy)
    const err = await login('nope').catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(401)
    expect(spy).not.toHaveBeenCalled()
    off()
  })

  it('reports 429 with a readable message', async () => {
    mockFetch(429, '', 'text/plain')
    await expect(login('x')).rejects.toMatchObject({ status: 429, message: expect.stringMatching(/Слишком много/) })
  })

  it('returns text bodies for cron actions', async () => {
    const fetch = mockFetch(200, 'parse ok', 'text/plain')
    const res = await runCron('rutracker', 'parselatest', { pages: '3' })
    expect(fetch.mock.calls[0][0]).toBe('/secret-panel/api/cron/rutracker/parselatest?pages=3')
    expect(res).toEqual({ data: 'parse ok', text: 'parse ok', status: 200 })
  })

  it('maps network failures to status 0', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('Failed to fetch'))))
    await expect(api('overview')).rejects.toMatchObject({ status: 0 })
  })
})
