// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { getBase } from './base.js'
import ru from '../lang/ru.js'

// Messages follow the panel language. `lang/index.jsx` imports this module (getLang), so the
// translator is injected from there instead of imported here (no import cycle).
let tg = (key, vars) => {
  let s = ru[key] == null ? key : ru[key]
  if (vars) for (const k in vars) s = String(s).split(`{${k}}`).join(vars[k])
  return s
}
export function setApiTranslator(fn) {
  tg = fn
}

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
  if (status === 404) return tg('api_not_found')
  if (status === 429) return tg('api_too_many')
  return tg('api_server_error', { status })
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
    throw new ApiError(tg('api_unreachable'), 0, null)
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

// --- UI language packs -------------------------------------------------------
// Runtime translation packs from the server's `Data/lang/*.json`. Public (works before login)
// so the login screen is localized too.
export const getLang = (opts) => get('lang', { ...opts, silent401: true })

// --- Dashboard / jobs --------------------------------------------------------
export const getOverview = (opts) => get('overview', opts)
export const getBackgroundJobs = (opts) => get('health/background-jobs', opts)
export const getParseAllStatus = (opts) => get('cron/maintenance/parseallstatus', opts)
export const resumeParseAll = () => get('cron/maintenance/resumeparseall', { raw: true })

// --- Sync ----------------------------------------------------------------------
export const getSyncPeers = (opts) => get('cron/sync/peers', opts)
export const startSyncCheck = () => get('cron/sync/check', { raw: true })
export const getSyncCheckStatus = (opts) => get('cron/sync/checkstatus', opts)

// --- Health / notifications ------------------------------------------------------
export const checkTrackerLogin = (slug) => post('trackers/checklogin', undefined, { query: { tracker: slug } })
export const sendTestNotification = () => post('notify/test')
export const muteIssue = (uid) => post('health/mute', undefined, { query: { uid } })
export const unmuteIssue = (uid) => post('health/unmute', undefined, { query: { uid } })
export const getHealthHistory = (limit = 50) => get('health/history', { query: { limit } })
export const getResources = (opts) => get('resources', opts)

// --- Data quality check (weekly cron, read-only) -----------------------------------
export const getDataCheck = (opts) => get('dev/checkdatastatus', opts)
export const runDataCheck = () => get('dev/checkdata', { raw: true })
export const startFixAll = () => get('dev/fixall')

// --- Trackers ------------------------------------------------------------
export const runCron = (slug, action, query) =>
  get(`cron/${encodeURIComponent(slug)}/${encodeURIComponent(action)}`, { query, raw: true })

// --- Logs --------------------------------------------------------------------
export const getLogs = (opts) => get('logs', opts)
export const getLog = (name, lines, opts) => get(`logs/${encodeURIComponent(name)}`, { ...opts, query: { lines } })
export const getFdbLog = (opts) => get('logs/fdb', opts)
export const setFdbLog = (patch) => post('logs/fdb', patch).then(ensureOk)
export const clearFdbLog = () => post('logs/fdb/clear').then(ensureOk)

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
    throw new ApiError(res.error || tg('api_op_failed'), 200, res)
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
export const getWafBots = (opts) => get('waf/bots', opts)
export const setWafBotCategory = (id, block) => post('waf/bots/category', { id, block }).then(ensureOk)
export const addWafBotRule = (entry) => post('waf/bots/rules', entry).then(ensureOk)
export const deleteWafBotRule = (list, value) => del('waf/bots/rules', { query: { list, value } }).then(ensureOk)
export const setWafRobots = (disallow) => post('waf/bots/robots', { disallow }).then(ensureOk)
