import { getBase } from './base.js'

export class ApiError extends Error {
  constructor(message, status, body) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.body = body
  }
}

const unauthorizedListeners = new Set()

/** Subscribe to 401 responses from protected endpoints (→ show login). */
export function onUnauthorized(fn) {
  unauthorizedListeners.add(fn)
  return () => unauthorizedListeners.delete(fn)
}

function emitUnauthorized() {
  for (const fn of unauthorizedListeners) {
    try {
      fn()
    } catch {
      /* listener errors must not break the request flow */
    }
  }
}

export function apiUrl(path, query) {
  const clean = String(path).replace(/^\/+/, '')
  let url = `${getBase()}/api/${clean}`
  if (query) {
    const qs = new URLSearchParams()
    for (const [k, v] of Object.entries(query)) {
      if (v !== undefined && v !== null && v !== '') qs.set(k, String(v))
    }
    const s = qs.toString()
    if (s) url += (url.includes('?') ? '&' : '?') + s
  }
  return url
}

async function readBody(res) {
  const text = await res.text()
  const ct = res.headers.get('content-type') || ''
  if (ct.includes('json') || /^\s*[[{]/.test(text)) {
    try {
      return { data: JSON.parse(text), text }
    } catch {
      /* not JSON after all */
    }
  }
  return { data: text, text }
}

function errorMessage(status, data) {
  if (data && typeof data === 'object') {
    if (typeof data.error === 'string' && data.error) return data.error
    if (typeof data.message === 'string' && data.message) return data.message
  }
  if (typeof data === 'string' && data.trim() && data.length < 300) return data.trim()
  if (status === 404) return 'Не найдено (404)'
  if (status === 429) return 'Слишком много попыток, подождите'
  return `Ошибка сервера (${status})`
}

/**
 * Fetch `{base}/api/{path}`. Always same-origin credentials and `X-Crab-Admin: 1`.
 * Resolves with parsed JSON (or text). Throws ApiError on non-2xx; a 401 also
 * notifies `onUnauthorized` listeners unless `silent401` is set.
 */
export async function api(path, { method = 'GET', query, body, signal, silent401 = false, raw = false } = {}) {
  const headers = { 'X-Crab-Admin': '1', Accept: 'application/json, text/plain, */*' }
  const init = { method, headers, credentials: 'same-origin', signal, cache: 'no-store' }
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json'
    init.body = typeof body === 'string' ? body : JSON.stringify(body)
  }
  let res
  try {
    res = await fetch(apiUrl(path, query), init)
  } catch (err) {
    if (err?.name === 'AbortError') throw err
    throw new ApiError('Сервер недоступен', 0, null)
  }
  const { data, text } = await readBody(res)
  if (!res.ok) {
    if (res.status === 401 && !silent401) emitUnauthorized()
    throw new ApiError(errorMessage(res.status, data), res.status, data)
  }
  return raw ? { data, text, status: res.status } : data
}

export const get = (path, opts) => api(path, { ...opts, method: 'GET' })
export const post = (path, body, opts) => api(path, { ...opts, method: 'POST', body })

// --- Session ---------------------------------------------------------------
export const getSession = () => get('session', { silent401: true })
export const login = (devkey) => post('login', { devkey }, { silent401: true })
export const logout = () => post('logout', undefined, { silent401: true })

// --- Dashboard / jobs --------------------------------------------------------
export const getOverview = (opts) => get('overview', opts)
export const getBackgroundJobs = (opts) => get('health/background-jobs', opts)
export const getParseAllStatus = (opts) => get('cron/maintenance/parseallstatus', opts)
export const resumeParseAll = () => get('cron/maintenance/resumeparseall', { raw: true })

// --- Trackers ------------------------------------------------------------
export const runCron = (slug, action, query) =>
  get(`cron/${encodeURIComponent(slug)}/${encodeURIComponent(action)}`, { query, raw: true })

// --- Logs --------------------------------------------------------------------
export const getLogs = (opts) => get('logs', opts)
export const getLog = (name, lines, opts) => get(`logs/${encodeURIComponent(name)}`, { ...opts, query: { lines } })

// --- Config ------------------------------------------------------------------
export const getConfig = (format) => get('config', { query: { format } })
export const getConfigSchema = () => get('config/schema')
export const validateConfig = (payload) => post('config/validate', payload)
export const diffConfig = (payload) => post('config/diff', payload)
export const renderConfig = (payload) => post('config/render', payload)
export const parseConfig = (payload) => post('config/parse', payload)
export const formatConfig = (payload) => post('config/format', payload)
export const saveConfig = (payload) => post('config', payload)

// --- Self-update ---------------------------------------------------------------
export const getUpdate = (force, opts) => get('update', { ...opts, query: force ? { force: 1 } : undefined })
export const applyUpdate = () => post('update/apply').then(ensureOk)

// --- Cloudflare bypass (FlareSolverr / cffetch) --------------------------------
export const getCloudflareStatus = (opts) => get('cron/cloudflare/status', opts)
export const closeBrowserSessions = (host) => post('cron/cloudflare/sessions/close', undefined, { query: { host } }).then(ensureOk)
export const pauseCloudflare = (value) => post('cron/cloudflare/pause', undefined, { query: { value } }).then(ensureOk)
export const resetCloudflareStats = () => post('cron/cloudflare/stats/reset').then(ensureOk)

// --- Maintenance / dev ---------------------------------------------------------
export const runPath = (path, query) => get(path, { query, raw: true })

// --- WAF ---------------------------------------------------------------------
export const del = (path, opts) => api(path, { ...opts, method: 'DELETE' })

/**
 * Some endpoints answer `{ ok:false, error }` with a 2xx status - turn that into
 * an ApiError so callers can rely on try/catch alone.
 */
export function ensureOk(res) {
  if (res && typeof res === 'object' && !Array.isArray(res) && res.ok === false) {
    throw new ApiError(res.error || 'Операция не выполнена', 200, res)
  }
  return res
}

export const getWafOverview = (window = '60m', opts) => get('waf/overview', { ...opts, query: { window } })
export const getWafRequests = (query, opts) => get('waf/requests', { ...opts, query })
export const getWafIps = (query, opts) => get('waf/ips', { ...opts, query })
export const getWafRules = (opts) => get('waf/rules', opts)
export const addWafRule = (entry) => post('waf/rules', entry).then(ensureOk)
export const deleteWafRule = (list, value) => del('waf/rules', { query: { list, value } }).then(ensureOk)
export const banWafIp = (payload) => post('waf/ban', payload).then(ensureOk)
export const unbanWafIp = (ip) => del('waf/ban', { query: { ip } }).then(ensureOk)
export const resetWafStats = () => post('waf/reset').then(ensureOk)
