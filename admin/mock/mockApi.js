// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

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
  syncCheckRunning: false,
  mutedIssues: new Set(),
  fixAll: null,
  syncKeys: [
    { name: 'home-box', key: 'q7Fz…9kLm', createdAt: new Date(Date.now() - 20 * 86_400_000).toISOString(), lastUsedAt: new Date(Date.now() - 6 * 60_000).toISOString(), lastIp: '94.156.102.20', disabled: false },
    { name: 'old-vps', key: 'Ab12…Zz09', createdAt: new Date(Date.now() - 90 * 86_400_000).toISOString(), lastUsedAt: new Date(Date.now() - 40 * 86_400_000).toISOString(), lastIp: '203.0.113.5', disabled: true },
  ],
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

const AUTH_TRACKERS = ['kinozal', 'selezen', 'anifilm', 'mazepa', 'toloka', 'baibako', 'animelayer', 'korsars', 'rudub']
const loginState = { kinozal: { ok: true, at: new Date(Date.now() - 3_600_000).toISOString(), error: '' }, selezen: { ok: false, at: new Date(Date.now() - 600_000).toISOString(), error: 'TakeLogin failed: no PHPSESSID (403)' } }
function trackerLogin(slug) {
  if (!AUTH_TRACKERS.includes(slug)) return { required: false }
  const t = config[slug[0].toUpperCase() + slug.slice(1)] || {}
  const configured = !!(t.cookie || (t.login && t.login.u))
  return { required: true, configured, canCheck: ['kinozal', 'selezen', 'anifilm'].includes(slug), status: loginState[slug] || null }
}

function fixAllStatus() {
  const st = state.fixAll || { running: false, steps: [] }
  return st.running ? st : { ...st, plan: ['dev/fixrutrackerdomainduplicates', 'dev/fixslugduplicates', 'dev/fixrutrackernames', 'dev/fixzerosizes'] }
}

function dataCheckReport() {
  const row = (tracker, rows, zeroSize, dupIds, foreignHost, badNames) => ({ tracker, rows, zeroSize, dupIds, foreignHost, badNames, issues: zeroSize + dupIds + foreignHost + badNames })
  const trackers = [row('rutracker', 928294, 505, 1214, 3, 21044), row('kinozal', 555321, 0, 651, 0, 0), row('rutor', 523139, 0, 49, 0, 0), row('toloka', 57906, 36, 0, 0, 0), row('nnmclub', 145939, 0, 0, 0, 0)]
  const total = trackers.reduce((a, r) => ({ rows: a.rows + r.rows, zeroSize: a.zeroSize + r.zeroSize, dupIds: a.dupIds + r.dupIds, foreignHost: a.foreignHost + r.foreignHost, badNames: a.badNames + r.badNames, issues: a.issues + r.issues }), { rows: 0, zeroSize: 0, dupIds: 0, foreignHost: 0, badNames: 0, issues: 0 })
  return {
    ok: true,
    at: new Date(Date.now() - 2 * 86_400_000).toISOString(),
    tookSec: 412,
    buckets: 599_600,
    total,
    trackers,
    fixes: { zeroSize: 'dev/fixzerosizes', dupIds: 'dev/fixslugduplicates', badNames: 'dev/fixrutrackernames', foreignHost: { kinozal: 'dev/fixkinozaldomainduplicates', rutracker: 'dev/fixrutrackerdomainduplicates', selezen: 'dev/fixselezendomainduplicates', ultradox: 'dev/fixultradoxdomainduplicates' } },
    trigger: 'fix_slug_duplicates',
    previous: {
      at: new Date(Date.now() - 3 * 86_400_000).toISOString(),
      trigger: 'cron',
      total: { rows: 2_281_228, zeroSize: 505, dupIds: 20_515, foreignHost: 3, badNames: 217_050, issues: 238_073 },
      trackers: [row('rutracker', 928294, 505, 1214, 3, 217050), row('kinozal', 555321, 0, 7171, 0, 0), row('rutor', 523139, 0, 12681, 0, 0), row('toloka', 57906, 36, 0, 0, 0), row('nnmclub', 145939, 0, 320, 0, 0)],
    },
  }
}

function syncPeers() {
  const now = Date.now()
  const ft = (msAgo) => (BigInt(Math.floor((now - msAgo) / 1000)) + 11644473600n) * 10000000n
  return [
    { ip: '94.156.102.20', name: 'home-box', version: '1.2.3', firstSeen: new Date(now - 40 * 86_400_000).toISOString(), lastSeen: new Date(now - 6 * 60_000).toISOString(), requests: 48_211, lastCursor: Number(ft(9 * 60_000)), lastSpidr: new Date(now - 3 * 3_600_000).toISOString(), lastCheck: new Date(now - 20 * 3_600_000).toISOString(), lastRefetch: new Date(now - 6 * 60_000).toISOString(), status: { buckets: 610_834, issues: 1, errors: 0, idIndex: true, check: { at: new Date(now - 20 * 3_600_000).toISOString(), ok: true, remaining: 0, missing: 140, mismatched: 119, extra: 2 } }, statusAt: new Date(now - 6 * 60_000).toISOString() },
    { ip: '2a01:4f8:c0c:1234::1', version: '1.0.8', firstSeen: new Date(now - 12 * 86_400_000).toISOString(), lastSeen: new Date(now - 5 * 3_600_000).toISOString(), requests: 3_902, lastCursor: Number(ft(5 * 3_600_000)), lastSpidr: null, lastCheck: null, lastRefetch: null },
    { ip: '203.0.113.77', version: '', firstSeen: new Date(now - 3 * 86_400_000).toISOString(), lastSeen: new Date(now - 26 * 3_600_000).toISOString(), requests: 120, lastCursor: 0, lastSpidr: null, lastCheck: null, lastRefetch: null },
  ]
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
      login: trackerLogin(slug),
    })),
    issues: [
      { id: 'login_failed', key: 'selezen', severity: 'error', link: '/trackers', params: { tracker: 'selezen', error: 'TakeLogin failed: no PHPSESSID (403)' } },
      { id: 'tracker_stale', key: 'baibako', severity: 'warn', link: '/trackers', params: { tracker: 'baibako', days: 385 } },
      { id: 'sync_check_backlog', key: '', severity: 'warn', link: '/', params: { remaining: 4120 } },
    ].map((i) => ({ ...i, muted: state.mutedIssues.has(i.key ? `${i.id}:${i.key}` : i.id) })),
    notifyConfigured: !!(config.notify && (config.notify.webhookUrl || config.notify.telegramToken)),
    sync: {
      enabled: !!config.syncapi,
      syncapi: config.syncapi,
      lastsync: new Date(Date.now() - 45 * 60_000).toISOString(),
      starsync: null,
      torrents: 3_342_561,
      remoteTorrents: 3_620_842,
      check: { at: new Date(Date.now() - 5 * 3_600_000).toISOString(), ok: true, tookSec: 412, hostBuckets: 603_000, localBuckets: 599_600, missing: 3_400, mismatched: 12_800, extra: 190, fetchedBuckets: 16_200, importedRows: 41_300, prunedRows: 880, deletedBuckets: 150, keptLocalBuckets: 40, remaining: 0 },
      checkRunning: !!state.syncCheckRunning,
      checkProgress: state.syncCheckRunning ? { startedAt: new Date(Date.now() - 20_000).toISOString(), phase: 'fetch', done: 118, total: 258, missing: 140, mismatched: 119, extra: 2 } : null,
      checkMinutes: 1440,
    },
    config: { path: 'init.yaml', format: 'yaml' },
    hints: [{ id: 'evercache_off' }, { id: 'stats_too_frequent', minutes: 15 }],
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
  if (path === 'stats/history') {
    const n = Number(query.get('days') || 30)
    const days = Array.from({ length: n }, (_, i) => new Date(Date.now() - (n - 1 - i) * 86_400_000).toISOString().slice(0, 10))
    const series = (base, jitter) => days.map((d, i) => (i === 3 ? null : Math.max(0, Math.round(base + Math.sin(i / 3) * jitter + (i % 7 === 0 ? jitter : 0)))))
    return send(res, 200, { days, trackers: { rutor: { new: series(380, 120), all: [] }, rutracker: { new: series(900, 300), all: [] }, kinozal: { new: series(420, 150), all: [] }, nnmclub: { new: series(160, 60), all: [] }, selezen: { new: series(6, 5), all: [] }, toloka: { new: days.map(() => 0), all: [] } } })
  }
  if (path === 'resources') {
    const gb = 1024 ** 3
    return send(res, 200, {
      at: new Date().toISOString(),
      process: { rss: Math.round(1.9 * gb), cpuPercent: 37.5, cgroupUsage: Math.round(2.1 * gb), cgroupLimit: 4 * gb, cpus: 4 },
      host: { memTotal: 16 * gb, memAvailable: Math.round(6.4 * gb), load: [1.42, 1.1, 0.96], diskFree: 61 * gb, diskTotal: 150 * gb },
      docker: { available: true, containers: [
        { name: 'flaresolverr', image: 'ghcr.io/flaresolverr/flaresolverr:latest', state: 'running', status: 'Up 3 days', memUsage: Math.round(5.6 * gb), memLimit: 6 * gb, cpuPercent: 84.2 },
        { name: 'crabindex', image: 'ghcr.io/sheinices/crabindex:latest', state: 'running', status: 'Up 3 days', memUsage: Math.round(2.1 * gb), memLimit: 4 * gb, cpuPercent: 37.5 },
        { name: 'flaresolverr-crawl', image: 'ghcr.io/flaresolverr/flaresolverr:latest', state: 'running', status: 'Up 3 days', memUsage: Math.round(0.7 * gb), memLimit: 1 * gb, cpuPercent: 2.1 },
        { name: 'cffetch', image: 'ghcr.io/jacred-fdb/cffetch:latest', state: 'running', status: 'Up 3 days', memUsage: 48 * 1024 * 1024, memLimit: 256 * 1024 * 1024, cpuPercent: 0.3 },
        { name: 'warp', image: 'caomingjun/warp', state: 'exited', status: 'Exited (1) 2 hours ago' },
      ] },
    })
  }
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
  if (path === 'trackers/checklogin' && method === 'POST') {
    const slug = (query.get('tracker') || '').toLowerCase()
    if (!['kinozal', 'selezen', 'anifilm'].includes(slug)) return send(res, 404, { ok: false, error: 'no login check for this tracker' })
    const ok = slug !== 'selezen'
    loginState[slug] = { ok, at: new Date().toISOString(), error: ok ? '' : 'TakeLogin failed: no PHPSESSID (403)' }
    return send(res, 200, ok ? { ok: true, tracker: slug, status: loginState[slug] } : { ok: false, tracker: slug, error: loginState[slug].error, status: loginState[slug] })
  }
  if (path === 'health/history') {
    const now = Date.now()
    const ev = (minAgo, event, id, key, severity, params, minutes) => ({ at: new Date(now - minAgo * 60_000).toISOString(), event, id, key, severity, params, ...(minutes == null ? {} : { minutes }) })
    return send(res, 200, { ok: true, events: [
      ev(12, 'resolved', 'fs_tab_crashes', 'open.selezen.org', 'warn', { host: 'open.selezen.org', crashes: 23 }, 310),
      ev(95, 'appeared', 'login_failed', 'selezen', 'error', { tracker: 'selezen', error: 'TakeLogin failed: no PHPSESSID (403)' }),
      ev(322, 'appeared', 'fs_tab_crashes', 'open.selezen.org', 'warn', { host: 'open.selezen.org', crashes: 20 }),
      ev(1440, 'resolved', 'sync_stale', '', 'warn', { minutes: 130, limit: 90 }, 40),
      ev(1480, 'appeared', 'sync_stale', '', 'warn', { minutes: 91, limit: 90 }),
    ].slice(0, Number(query.get('limit') || 50)) })
  }
  if ((path === 'health/mute' || path === 'health/unmute') && method === 'POST') {
    const uid = query.get('uid') || ''
    if (path === 'health/mute') state.mutedIssues.add(uid)
    else state.mutedIssues.delete(uid)
    return send(res, 200, { ok: true, uid, muted: path === 'health/mute' })
  }
  if (path === 'notify/test' && method === 'POST') return send(res, 200, { ok: false, errors: ['no channel configured (notify.telegramToken + telegramChatId or notify.webhookUrl)'] })
  if (path === 'cron/sync/peers') return send(res, 200, { ok: true, opensync: true, requireKey: false, staleHours: 6, peers: syncPeers() })
  if (path === 'cron/sync/keys') return send(res, 200, { ok: true, requireKey: false, keys: state.syncKeys })
  if (path === 'cron/sync/keys/create') {
    const name = (query.get('name') || '').trim()
    if (!name) return send(res, 200, { ok: false, error: 'name: 1-40 letters, digits, - _ . or space' })
    if (state.syncKeys.some((k) => k.name.toLowerCase() === name.toLowerCase())) return send(res, 200, { ok: false, error: 'a key with this name exists' })
    const key = Array.from({ length: 32 }, () => 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'[Math.floor(Math.random() * 62)]).join('')
    state.syncKeys.unshift({ name, key: `${key.slice(0, 4)}…${key.slice(-4)}`, createdAt: new Date().toISOString(), lastUsedAt: null, lastIp: '', disabled: false })
    return send(res, 200, { ok: true, name, key })
  }
  if (path === 'cron/sync/keys/revoke' || path === 'cron/sync/keys/enable' || path === 'cron/sync/keys/delete') {
    const name = query.get('name') || ''
    const k = state.syncKeys.find((x) => x.name === name)
    if (!k) return send(res, 200, { ok: false })
    if (path.endsWith('delete')) state.syncKeys = state.syncKeys.filter((x) => x !== k)
    else k.disabled = path.endsWith('revoke')
    return send(res, 200, { ok: true })
  }
  if (path === 'cron/sync/check') {
    state.syncCheckRunning = true
    setTimeout(() => { state.syncCheckRunning = false }, 8000)
    return send(res, 200, 'ok', 'text/plain; charset=utf-8')
  }
  if (path === 'cron/sync/checkstatus') return send(res, 200, { ok: true, running: !!state.syncCheckRunning, last: overview().sync.check })
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
  if (path === 'dev/checkdatastatus') return send(res, 200, { ok: true, running: false, last: dataCheckReport(), fixAll: fixAllStatus() })
  if (path === 'dev/fixall') {
    if (state.fixAll?.running) return send(res, 200, { ok: false, error: 'fix all is already running' })
    const steps = ['dev/fixrutrackerdomainduplicates', 'dev/fixslugduplicates', 'dev/fixrutrackernames', 'dev/fixzerosizes']
    state.fixAll = { running: true, startedAt: new Date().toISOString(), phase: 'fixing', steps: steps.map((p) => ({ path: p, status: 'pending' })) }
    steps.forEach((p, i) => {
      setTimeout(() => { state.fixAll.steps[i].status = 'running' }, i * 4000)
      setTimeout(() => { state.fixAll.steps[i] = { path: p, status: 'done', tookSec: 4, result: { ok: true, fixed: 12 * (i + 1) } } }, (i + 1) * 4000)
    })
    setTimeout(() => { state.fixAll.phase = 'checking' }, steps.length * 4000)
    setTimeout(() => { state.fixAll = { ...state.fixAll, running: false, phase: 'done', finishedAt: new Date().toISOString() } }, steps.length * 4000 + 5000)
    return send(res, 200, { ok: true, steps })
  }
  if (path === 'dev/fixallstatus') return send(res, 200, fixAllStatus())
  if (path === 'dev/checkdata') return send(res, 200, dataCheckReport())
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

  if (path === 'logs/fdb') {
    state.fdb = state.fdb || { enabled: false, retentionDays: 7, maxSizeMb: 1024, maxFiles: 0, files: 2, totalBytes: 48_234_112, oldest: '2026-09-25', newest: '2026-09-26' }
    if (req.method === 'POST') {
      const patch = await readJson(req)
      state.fdb = { ...state.fdb, ...patch }
      return send(res, 200, { ok: true, state: state.fdb })
    }
    return send(res, 200, state.fdb)
  }
  if (path === 'logs/fdb/clear') {
    const freed = state.fdb?.totalBytes || 0
    state.fdb = { ...(state.fdb || {}), files: 0, totalBytes: 0, oldest: null, newest: null }
    return send(res, 200, { ok: true, files: 2, bytes: freed })
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
