/**
 * Dev-only mock of the admin API (see ADMIN_CONTRACT.md). Mounted by
 * vite.config.js in `vite serve` only; never part of the production bundle.
 *
 * Login devkey: `devkey`. Base path: `/admin`.
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import YAML from 'yaml'
import { handleWaf } from './wafMock.js'

const here = dirname(fileURLToPath(import.meta.url))
const BASE = '/admin'
const SENSITIVE = ['apikey', 'devkey', 'cookie', 'u', 'p', 'username', 'password', 'token']

const schema = JSON.parse(readFileSync(join(here, 'fixtures/schema.json'), 'utf8'))
let config = JSON.parse(readFileSync(join(here, 'fixtures/config.json'), 'utf8'))
let configMtime = new Date().toISOString()

const startedAt = Date.now()
const state = {
  session: false,
  failures: [],
  checkRunning: false,
}

const TRACKERS = [
  ['rutracker', 'Rutracker'], ['rutor', 'Rutor'], ['kinozal', 'Kinozal'], ['nnmclub', 'NNMClub'], ['megapeer', 'Megapeer'],
  ['bitru', 'Bitru'], ['toloka', 'Toloka'], ['mazepa', 'Mazepa'], ['lostfilm', 'Lostfilm'], ['baibako', 'Baibako'],
  ['torrentby', 'TorrentBy'], ['selezen', 'Selezen'], ['animelayer', 'Animelayer'], ['anidub', 'Anidub'],
  ['anistar', 'Anistar'], ['anibelka', 'Anibelka'], ['aniliberty', 'Aniliberty'], ['knaben', 'Knaben'],
  ['leproduction', 'Leproduction'], ['viruseproject', 'Viruseproject'], ['anifilm', 'Anifilm'], ['korsars', 'Korsars'],
  ['ultradox', 'Ultradox'], ['rudub', 'Rudub'], ['subsplease', 'SubsPlease'],
]
const PARSE_ALL = new Set(['anibelka', 'kinozal', 'korsars', 'megapeer', 'nnmclub', 'rutor', 'rutracker', 'toloka', 'torrentby', 'ultradox'])

function jobs() {
  const elapsed = Math.floor((Date.now() - startedAt) / 1000)
  const done = Math.min(154, 6 + Math.floor(elapsed / 5))
  return [
    {
      id: 'rutracker:ParseAllTask',
      tracker: 'rutracker',
      job: 'ParseAllTask',
      startedAtUtc: new Date(startedAt - 940_000).toISOString(),
      elapsedSeconds: 940 + elapsed,
      pagesCompleted: done,
      pagesTotal: 154,
      percent: Math.round((done / 154) * 100),
      currentCategory: '32',
      currentPage: 5,
      summary: `${done}/154 pages · category 32 · page 5`,
    },
    {
      id: 'kinozal:UpdateTasksParse',
      tracker: 'kinozal',
      job: 'UpdateTasksParse',
      startedAtUtc: new Date(startedAt - 30_000).toISOString(),
      elapsedSeconds: 30 + elapsed,
      pagesCompleted: 0,
      pagesTotal: 0,
      percent: null,
      currentCategory: null,
      currentPage: null,
      summary: 'running',
    },
  ]
}

function cloudflareStatus() {
  const now = Date.now()
  const ago = (min) => new Date(now - min * 60_000).toISOString()
  const host = (name, over) => ({
    host: name,
    browserRequests: 0, browserOk: 0, browserFailed: 0, tabCrashed: 0, browserTimeouts: 0, challengeFailed: 0,
    sessionErrors: 0, unreachable: 0, pageFailed: 0, otherErrors: 0, sessionsCreated: 1, sessionsRecycled: 0,
    sessionsClosedIdle: 0, fastOk: 0, fastFailed: 0, clearanceRenewals: 0, totalBrowserMs: 0, maxBrowserMs: 0,
    lastOkAt: ago(3), lastErrorAt: null, lastError: null,
    ...over,
  })
  const crash = 'Error: Error solving the challenge. Message: tab crashed (Session info: chrome=152.0.7977.82)'
  return {
    enabled: true,
    paused: Boolean(state.cfPaused),
    settings: { url: 'http://127.0.0.1:8191/v1', crawlUrl: '', maxTimeoutMs: 300000, sessionIdleMinutes: 120, browserTimeoutRetries: 1, recycleAfterTimeouts: 3, guardedHours: 6, recheckMinutes: 30 },
    solver: { url: 'http://127.0.0.1:8191/v1', reachable: true, version: '3.5.2', userAgent: 'Mozilla/5.0 Chrome/152.0.0.0', message: 'FlareSolverr is ready!', sessions: ['crabindex-rutracker_org', 'crabindex-kinozal_guru', 'crabindex-open_selezen_org'] },
    crawlSolver: null,
    cffetch: {
      enabled: true, url: 'http://127.0.0.1:8192/fetch', impersonate: 'chrome136', maxConcurrent: 4, clearanceMinutes: 60,
      hosts: [
        { host: 'rutracker.org', clearanceAt: ago(12), blockedUntil: null },
        { host: 'kinozal.guru', clearanceAt: ago(40), blockedUntil: null },
      ],
    },
    sessions: [
      { name: 'crabindex-kinozal_guru', host: 'kinozal.guru', alive: true, busy: false, lastUse: ago(6), consecutiveTimeouts: 0 },
      { name: 'crabindex-open_selezen_org', host: 'open.selezen.org', alive: true, busy: true, lastUse: ago(0), consecutiveTimeouts: 0 },
      { name: 'crabindex-rutracker_org', host: 'rutracker.org', alive: true, busy: false, lastUse: ago(2), consecutiveTimeouts: 0 },
    ],
    guarded: [{ host: 'rutracker.org', since: ago(300) }, { host: 'kinozal.guru', since: ago(200) }, { host: 'open.selezen.org', since: ago(90) }],
    stats: {
      since: ago(720),
      hosts: [
        host('open.selezen.org', { browserRequests: 96, browserOk: 22, browserFailed: 74, tabCrashed: 70, browserTimeouts: 4, sessionsCreated: 76, sessionsRecycled: 74, fastOk: 310, fastFailed: 41, clearanceRenewals: 38, totalBrowserMs: 2_400_000, maxBrowserMs: 202_000, lastErrorAt: ago(9), lastError: crash }),
        host('rutracker.org', { browserRequests: 14, browserOk: 14, fastOk: 5210, fastFailed: 12, clearanceRenewals: 12, totalBrowserMs: 98_000, maxBrowserMs: 11_000, sessionsCreated: 2 }),
        host('kinozal.guru', { browserRequests: 6, browserOk: 5, browserFailed: 1, pageFailed: 1, fastOk: 1330, fastFailed: 3, totalBrowserMs: 54_000, maxBrowserMs: 14_000, lastErrorAt: ago(300), lastError: 'http 403' }),
      ],
      recentErrors: [
        { at: ago(9), host: 'open.selezen.org', kind: 'tabCrashed', message: crash },
        { at: ago(24), host: 'open.selezen.org', kind: 'tabCrashed', message: crash },
        { at: ago(41), host: 'open.selezen.org', kind: 'browserTimeout', message: "HTTPConnectionPool(host='localhost', port=55127): Read timed out. (read timeout=120)" },
        { at: ago(300), host: 'kinozal.guru', kind: 'pageFailed', message: 'http 403' },
      ],
    },
  }
}

function parseAllStatus() {
  return TRACKERS.filter(([s]) => PARSE_ALL.has(s)).map(([tracker]) => ({
    tracker,
    running: tracker === 'rutracker',
    pending: tracker === 'rutracker' ? 148 : tracker === 'kinozal' ? 12 : 0,
    mapCount: tracker === 'rutracker' ? 16230 : 420,
  }))
}

function overview() {
  const pa = new Map(parseAllStatus().map((p) => [p.tracker, p]))
  const disabled = new Set(config.disable_trackers || [])
  return {
    version: '1.4.0-dev',
    gitSha: 'a1b2c3d',
    buildDate: '2026-09-20T10:00:00Z',
    uptimeSeconds: Math.floor((Date.now() - startedAt) / 1000) + 273_600,
    listen: `${config.listenip}:${config.listenport}`,
    masterDbKeys: 1_284_512,
    torrents: 3_912_004,
    lastUpdateDb: new Date(Date.now() - 180_000).toISOString(),
    fastDbKeys: 412_880,
    activeJobs: jobs(),
    trackers: TRACKERS.map(([slug, name]) => ({
      slug,
      name,
      enabled: !disabled.has(slug),
      parseAll: pa.get(slug) ? { running: pa.get(slug).running, pending: pa.get(slug).pending, mapCount: pa.get(slug).mapCount } : null,
    })),
    sync: { enabled: !!config.syncapi, syncapi: config.syncapi, lastsync: null, starsync: null },
    config: { path: 'init.yaml', format: 'yaml' },
  }
}

const LOGS = ['app.log', 'cron.log', 'rutracker.log', 'kinozal.log', 'rutor.log', 'fdb.2026-09-25.log', 'sync.log']

function logLines(name, n) {
  const out = []
  const levels = ['INFO', 'INFO', 'INFO', 'WARN', 'DEBUG', 'ERROR']
  const now = Date.now()
  for (let i = n - 1; i >= 0; i--) {
    const lvl = levels[(i * 7 + name.length) % levels.length]
    const ts = new Date(now - i * 4000).toISOString()
    out.push(`${ts} ${lvl.padEnd(5)} ${name.replace('.log', '')}: page ${i % 50} parsed, added=${i % 7} updated=${i % 3} skipped=${i % 11}`)
  }
  return out
}

function isSensitive(k) {
  return SENSITIVE.includes(String(k).toLowerCase())
}

function flatDiff(a, b, prefix = '') {
  const out = []
  const obj = (v) => v && typeof v === 'object' && !Array.isArray(v)
  if (obj(a) || obj(b)) {
    const keys = new Set([...Object.keys(obj(a) ? a : {}), ...Object.keys(obj(b) ? b : {})])
    for (const k of [...keys].sort()) out.push(...flatDiff(obj(a) ? a[k] : undefined, obj(b) ? b[k] : undefined, prefix ? `${prefix}.${k}` : k))
    return out
  }
  const sa = a === undefined || a === null ? null : typeof a === 'string' ? a : JSON.stringify(a)
  const sb = b === undefined || b === null ? null : typeof b === 'string' ? b : JSON.stringify(b)
  if (sa === sb) return out
  out.push({
    path: prefix,
    oldValue: sa,
    newValue: sb,
    sensitive: isSensitive(prefix.split('.').pop()),
    change: sa == null ? 'added' : sb == null ? 'removed' : 'modified',
  })
  return out
}

function validate(data) {
  const errors = []
  const warnings = []
  if (!data || typeof data !== 'object') errors.push('Корень конфига должен быть объектом')
  else {
    const port = Number(data.listenport)
    if (data.listenport != null && !(port >= 1 && port <= 65535)) errors.push('listenport: 1-65535')
    if (data.tracksmod != null && ![0, 1].includes(Number(data.tracksmod))) errors.push('tracksmod: допустимы только 0 или 1')
    if (data.admin?.token && !/^[A-Za-z0-9]{18}$/.test(data.admin.token)) errors.push('admin.token: ровно 18 символов [A-Za-z0-9]')
    if (data.admin?.path && !/^\/?[a-z0-9_-]{2,32}$/.test(data.admin.path)) errors.push('admin.path: некорректный путь')
    if (!data.apikey) warnings.push('apikey пуст - поиск доступен без ключа')
  }
  return { ok: errors.length === 0, error: errors[0] ?? null, errors, warnings }
}

function render(data, format) {
  return format === 'json' ? JSON.stringify(data, null, 2) : YAML.stringify(data)
}

function parseContent(content, format) {
  const t = String(content || '').trim()
  if (format === 'json' || /^[{[]/.test(t)) return JSON.parse(t)
  return YAML.parse(t)
}

function resolvePayload(body) {
  if (body?.data && typeof body.data === 'object') return body.data
  if (body?.content) return parseContent(body.content, body.format)
  throw new Error('Укажите data или content')
}

function send(res, status, payload, type = 'application/json; charset=utf-8') {
  res.statusCode = status
  res.setHeader('Content-Type', type)
  res.setHeader('Cache-Control', 'no-store')
  res.end(typeof payload === 'string' ? payload : JSON.stringify(payload))
}

function readJson(req) {
  return new Promise((resolve) => {
    let buf = ''
    req.on('data', (c) => (buf += c))
    req.on('end', () => {
      try {
        resolve(buf ? JSON.parse(buf) : null)
      } catch {
        resolve(null)
      }
    })
  })
}

const delay = (ms) => new Promise((r) => setTimeout(r, ms))

async function handle(req, res, path, query) {
  const method = req.method || 'GET'
  if (method !== 'GET' && req.headers['x-crab-admin'] !== '1') return send(res, 403, { error: 'missing X-Crab-Admin header' })

  if (path === 'session') return send(res, 200, { authenticated: state.session, version: '1.4.0-dev', loginEnabled: true })
  if (path === 'login' && method === 'POST') {
    const now = Date.now()
    state.failures = state.failures.filter((t) => now - t < 600_000)
    if (state.failures.length >= 5) return send(res, 429, { ok: false, error: 'too many attempts' })
    const body = await readJson(req)
    await delay(300)
    if (body?.devkey === config.devkey) {
      state.session = true
      res.setHeader('Set-Cookie', `crab_session=mock; Path=${BASE}; HttpOnly; SameSite=Strict`)
      return send(res, 200, { ok: true })
    }
    state.failures.push(now)
    return send(res, 401, { ok: false, error: 'invalid key' })
  }
  if (path === 'logout' && method === 'POST') {
    state.session = false
    return send(res, 200, { ok: true })
  }

  if (!state.session) return send(res, 401, { error: 'unauthorized' })

  const waf = await handleWaf({ method, path, query, config, readBody: () => readJson(req) })
  if (waf) return send(res, waf[0], waf[1])

  if (path === 'overview') return send(res, 200, overview())
  if (path === 'update') {
    return send(res, 200, {
      current: '1.4.0-dev',
      available: true,
      canSelfUpdate: true,
      reason: null,
      latest: {
        tag: 'v1.5.0',
        version: '1.5.0',
        name: 'v1.5.0',
        publishedAt: new Date(Date.now() - 86_400_000).toISOString(),
        notes: '## Что нового\n- Раздел FlareSolverr в админ-панели\n- Обновление из панели\n',
        url: 'https://github.com/sheinices/crabindex/releases/tag/v1.5.0',
        assets: [],
        checkedAt: new Date().toISOString(),
      },
      asset: { name: 'crabindex-linux-x86_64.tar.gz', url: '#', size: 17_000_000 },
      state: { stage: 'idle', message: '', target: null, startedAt: null },
    })
  }
  if (path === 'update/apply') return send(res, 200, { ok: false, error: 'В режиме разработки обновление не выполняется' })
  if (path === 'health/background-jobs') return send(res, 200, { jobs: jobs() })
  if (path === 'cron/maintenance/parseallstatus') return send(res, 200, parseAllStatus())
  if (path === 'cron/maintenance/resumeparseall') return send(res, 200, 'resumed: kinozal (12 pending)', 'text/plain; charset=utf-8')
  if (path === 'cron/maintenance/status') return send(res, 200, { ok: true, running: state.checkRunning })
  if (path === 'cron/maintenance/check') {
    const mode = query.get('mode') || 'report'
    state.checkRunning = mode !== 'report'
    setTimeout(() => (state.checkRunning = false), 8000)
    await delay(600)
    return send(res, 200, { ok: true, mode, buckets: 1_284_512, corrupt: 0, emptySearchFields: 3, duplicates: 12, fixed: mode === 'report' ? 0 : 15, elapsedMs: 5821 })
  }
  if (path === 'cron/cloudflare/warmup') {
    await delay(800)
    return send(res, 200, 'warmup ok: crabindex-rutracker_org (cf_clearance valid 29m)', 'text/plain; charset=utf-8')
  }
  if (path === 'cron/cloudflare/status') return send(res, 200, cloudflareStatus())
  if (path === 'cron/cloudflare/sessions/close') {
    await delay(400)
    return send(res, 200, { ok: true, closed: 2, busy: 1 })
  }
  if (path === 'cron/cloudflare/pause') {
    state.cfPaused = query.get('value') !== 'false'
    return send(res, 200, { ok: true, paused: state.cfPaused, closed: state.cfPaused ? 3 : 0, busy: 0 })
  }
  if (path === 'cron/cloudflare/stats/reset') return send(res, 200, { ok: true })
  if (path === 'jsondb/save') return send(res, 200, 'work', 'text/plain; charset=utf-8')
  if (path.startsWith('dev/')) {
    await delay(700)
    const name = path.slice(4)
    return send(res, 200, { ok: true, job: name, scanned: 1_284_512, affected: name.startsWith('find') ? 3 : 0, samples: name.startsWith('find') ? ['rutracker:film:2024', 'kinozal:serial:2023'] : [] })
  }
  const cron = path.match(/^cron\/([a-z0-9]+)\/([a-z]+)$/)
  if (cron) {
    await delay(500)
    const [, slug, action] = cron
    if (!TRACKERS.some(([s]) => s === slug)) return send(res, 404, 'not found', 'text/plain')
    if (action.endsWith('status') || action === 'stats') return send(res, 200, { ok: true, tracker: slug, running: false, cursor: 42 })
    return send(res, 200, `${action} ${slug}: ok, added=12, updated=3, skipped=40 (${[...query].map(([k, v]) => `${k}=${v}`).join(' ') || 'defaults'})`, 'text/plain; charset=utf-8')
  }

  if (path === 'logs') {
    return send(res, 200, LOGS.map((name, i) => ({ name, size: 1024 * (50 + i * 731), modified: new Date(Date.now() - i * 3_600_000).toISOString() })))
  }
  const log = path.match(/^logs\/([a-z0-9._-]+\.log)$/)
  if (log) {
    if (!LOGS.includes(log[1])) return send(res, 404, { error: 'not found' })
    const n = Math.min(5000, Math.max(1, Number(query.get('lines')) || 300))
    return send(res, 200, { name: log[1], lines: logLines(log[1], n) })
  }

  if (path === 'config/schema') return send(res, 200, { ok: true, schema })
  if (path === 'config' && method === 'GET') {
    const fmt = query.get('format') || 'yaml'
    return send(res, 200, {
      ok: true, path: 'init.yaml', format: 'yaml', displayFormat: fmt, exists: true, lastModifiedUtc: configMtime,
      data: config, content: render(config, fmt), schema, examplePath: 'Data/example.yaml', sensitiveFields: SENSITIVE,
    })
  }
  if (path.startsWith('config') && method === 'POST') {
    const body = await readJson(req)
    try {
      if (path === 'config/parse') return send(res, 200, { ok: true, data: parseContent(body?.content, body?.format) })
      if (path === 'config/render') return send(res, 200, { ok: true, content: render(body?.data || {}, body?.format || 'yaml'), format: body?.format || 'yaml' })
      const data = resolvePayload(body)
      const v = validate(data)
      if (path === 'config/validate') return send(res, 200, v)
      if (path === 'config/diff') {
        const diffs = flatDiff(config, data)
        return send(res, 200, { ok: true, diffs, changeCount: diffs.length, validation: v })
      }
      if (path === 'config/format') {
        if (!v.ok) return send(res, 200, { ok: false, error: v.error })
        return send(res, 200, { ok: true, data, content: render(data, body?.format || 'yaml'), format: body?.format || 'yaml' })
      }
      if (path === 'config') {
        if (!v.ok) return send(res, 200, { ok: false, error: v.error })
        config = data
        configMtime = new Date().toISOString()
        return send(res, 200, { ok: true, path: 'init.yaml', format: 'yaml', lastModifiedUtc: configMtime, message: 'Конфигурация сохранена. Изменения применятся автоматически.' })
      }
    } catch (e) {
      return send(res, 200, { ok: false, error: String(e?.message || e) })
    }
  }
  return send(res, 404, 'Not Found', 'text/plain')
}

export function mockApiPlugin() {
  return {
    name: 'crab-admin-mock-api',
    apply: 'serve',
    transformIndexHtml(html) {
      return html.replace('<head>', `<head>\n    <meta name="crab-admin-base" content="${BASE}">`)
    },
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = new URL(req.url || '/', 'http://localhost')
        if (url.pathname === '/' || url.pathname === BASE) {
          res.statusCode = 302
          res.setHeader('Location', `${BASE}/`)
          return res.end()
        }
        if (url.pathname.startsWith(`${BASE}/api/`)) {
          const path = url.pathname.slice(BASE.length + 5).replace(/\/+$/, '')
          handle(req, res, path, url.searchParams).catch((e) => send(res, 500, { error: String(e) }))
          return
        }
        // Static files under the base (public/ assets such as the logo).
        if (url.pathname.startsWith(`${BASE}/`) && /\.[a-z0-9]+$/i.test(url.pathname) && !url.pathname.includes('/src/')) {
          req.url = url.pathname.slice(BASE.length) + url.search
          return next()
        }
        // SPA routes under the base: rewrite to the root index.html.
        if (url.pathname.startsWith(`${BASE}/`) && !/\.[a-z0-9]+$/i.test(url.pathname) && (req.headers.accept || '').includes('text/html')) {
          req.url = '/'
        }
        next()
      })
    },
  }
}
