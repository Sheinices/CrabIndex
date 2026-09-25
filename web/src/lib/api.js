/** Thin client for the public CrabIndex HTTP API (same origin). */

export class ApiError extends Error {
  constructor(status, message) {
    super(message || `HTTP ${status}`)
    this.name = 'ApiError'
    this.status = status
  }

  get unauthorized() {
    return this.status === 401 || this.status === 403
  }
}

/** Builds `path?query`, skipping empty values. Arrays are joined with commas. */
export function buildUrl(path, params = {}) {
  const qs = new URLSearchParams()
  for (const [key, raw] of Object.entries(params)) {
    const value = Array.isArray(raw) ? raw.filter(Boolean).join(',') : raw
    if (value === undefined || value === null || value === '' || value === false) continue
    qs.append(key, String(value))
  }
  const query = qs.toString()
  return query ? `${path}?${query}` : path
}

export async function fetchJson(url, { signal } = {}) {
  let res
  try {
    res = await fetch(url, { signal, headers: { Accept: 'application/json' } })
  } catch (err) {
    if (err?.name === 'AbortError') throw err
    throw new ApiError(0, 'network')
  }
  if (!res.ok) throw new ApiError(res.status)
  try {
    return await res.json()
  } catch {
    throw new ApiError(res.status, 'invalid json')
  }
}

/** `{ configured, apikey, version }` - whether the server needs a key and whether ours is valid. */
export function getConf(apikey, opts) {
  return fetchJson(buildUrl('/api/v1.0/conf', { apikey }), opts)
}

/** Native search. Returns an array (never null). */
export async function searchTorrents(query, apikey, opts) {
  const search = String(query || '').trim()
  if (search.length < 2) return []
  const data = await fetchJson(buildUrl('/api/v1.0/torrents', { search, apikey }), opts)
  return Array.isArray(data) ? data : []
}

export async function getTrackerStats(apikey, opts) {
  const data = await fetchJson(buildUrl('/stats/torrents', { apikey }), opts)
  return Array.isArray(data) ? data : []
}

export function getStatsMeta(apikey, opts) {
  return fetchJson(buildUrl('/stats/meta', { apikey }), opts)
}

export function getLastUpdateDb(opts) {
  return fetchJson('/lastupdatedb', opts)
}

export function getVersion(opts) {
  return fetchJson('/version', opts)
}
