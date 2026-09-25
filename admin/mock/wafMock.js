/**
 * Dev-only WAF mock (see WAF_CONTRACT.md): a synthetic request log that keeps
 * growing while the dev server runs, dynamic lists/bans and all `waf/*`
 * endpoints including the self-protection error.
 */
import { coversIp, isIpOrCidr } from '../src/lib/waf.js'

const MIN = 60_000
export const YOU = '192.168.1.10'

const UA = {
  lampa: 'Mozilla/5.0 (Linux; Android 12; SHIELD Android TV) AppleWebKit/537.36 Lampa/2.3.1',
  jackett: 'Jackett/0.22.1880',
  prowlarr: 'Prowlarr/1.24.3.4754 (ubuntu 24.04)',
  curl: 'curl/8.9.1',
  chrome: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36',
  sqlmap: 'sqlmap/1.8.9#stable (https://sqlmap.org)',
  nikto: 'Mozilla/5.00 (Nikto/2.5.0) (Evasions:None) (Test:Port Check)',
  bot: 'Mozilla/5.0 (compatible; zgrab/0.x)',
}

const API = ['/api/v1.0/torrents', '/api/v2.0/indexers/all/results', '/api/v1.0/torrents', '/api/v2.0/indexers/rutracker/results/torznab', '/lastupdatedb', '/api/v1.0/conf']
const TRAPS = ['/.env', '/wp-login.php', '/.git/config', '/phpmyadmin/index.php', '/wp-admin/setup-config.php']

// weight = share of traffic, kind drives status/blocked outcome.
const CLIENTS = [
  { ip: YOU, ua: UA.chrome, weight: 6, kind: 'admin' },
  { ip: '10.0.0.5', ua: UA.curl, weight: 4, kind: 'cron' },
  { ip: '198.51.100.23', ua: UA.lampa, weight: 22, kind: 'api' },
  { ip: '198.51.100.77', ua: UA.jackett, weight: 14, kind: 'api' },
  { ip: '2001:db8:85a3::8a2e:370:7334', ua: UA.prowlarr, weight: 10, kind: 'api' },
  { ip: '93.184.216.34', ua: UA.lampa, weight: 9, kind: 'api' },
  { ip: '5.188.62.140', ua: UA.curl, weight: 12, kind: 'flood' },
  { ip: '203.0.113.7', ua: UA.bot, weight: 5, kind: 'blacklisted' },
  { ip: '45.155.205.233', ua: UA.sqlmap, weight: 3, kind: 'ua' },
  { ip: '91.240.118.172', ua: UA.nikto, weight: 3, kind: 'trap' },
  { ip: '185.220.101.4', ua: UA.bot, weight: 2, kind: 'trap' },
  { ip: '192.0.2.10', ua: UA.curl, weight: 4, kind: 'banned' },
]
const TOTAL_WEIGHT = CLIENTS.reduce((a, c) => a + c.weight, 0)

let seed = 42
function rnd() {
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
}
const pick = (arr) => arr[Math.floor(rnd() * arr.length)]

function pickClient() {
  let r = rnd() * TOTAL_WEIGHT
  for (const c of CLIENTS) if ((r -= c.weight) <= 0) return c
  return CLIENTS[0]
}

function makeRequest(time) {
  const c = pickClient()
  const base = { time: new Date(time).toISOString(), ip: c.ip, method: 'GET', ua: c.ua, ms: Math.round(4 + rnd() * 60), blocked: null }
  switch (c.kind) {
    case 'admin':
      return { ...base, path: pick(['/admin/api/overview', '/admin/api/waf/overview', '/admin/api/health/background-jobs', '/admin/']), status: 200 }
    case 'cron':
      return { ...base, path: pick(['/cron/rutracker/parselatest', '/cron/kinozal/parselatest', '/jsondb/save']), status: 200, ms: Math.round(200 + rnd() * 4000) }
    case 'api': {
      const r = rnd()
      return { ...base, path: pick(API), status: r < 0.9 ? 200 : r < 0.96 ? 404 : r < 0.99 ? 400 : 500, ms: Math.round(8 + rnd() * 180) }
    }
    case 'flood':
      return rnd() < 0.55
        ? { ...base, path: '/api/v1.0/torrents', status: 429, blocked: 'rate', ms: 0 }
        : { ...base, path: '/api/v1.0/torrents', status: 403, blocked: 'ban', ms: 0 }
    case 'blacklisted':
      return { ...base, path: pick(['/', '/api/v1.0/torrents', '/robots.txt']), status: 403, blocked: 'blacklist', ms: 0 }
    case 'ua':
      return { ...base, path: pick(API), method: pick(['GET', 'POST']), status: 403, blocked: rnd() < 0.3 ? 'ua' : 'ban', ms: 0 }
    case 'trap':
      return rnd() < 0.4 ? { ...base, path: pick(TRAPS), status: 404, blocked: 'trap', ms: 0 } : { ...base, path: pick(TRAPS), status: 403, blocked: 'ban', ms: 0 }
    case 'banned':
      return { ...base, path: pick(API), status: 403, blocked: 'ban', ms: 0 }
    default:
      return { ...base, path: '/', status: 200 }
  }
}

const iso = (t) => new Date(t).toISOString()

const now0 = Date.now()
const state = {
  since: iso(now0 - 26 * 60 * MIN),
  log: [], // oldest first
  lastTick: now0,
  blacklist: [
    { value: '203.0.113.7', comment: 'сканер, долбит /api', created: iso(now0 - 3 * 86_400_000), expires: null },
    { value: '100.64.0.0/10', comment: 'CGNAT спамер', created: iso(now0 - 86_400_000), expires: iso(now0 + 6 * 86_400_000) },
  ],
  whitelist: [
    { value: '198.51.100.0/24', comment: 'офис', created: iso(now0 - 10 * 86_400_000), expires: null },
    { value: '2001:db8::/32', comment: 'домашний IPv6', created: iso(now0 - 2 * 86_400_000), expires: null },
  ],
  bans: [
    { ip: '192.0.2.10', reason: 'rate', created: iso(now0 - 5 * MIN), expires: iso(now0 + 10 * MIN) },
    { ip: '5.188.62.140', reason: 'rate', created: iso(now0 - 2 * MIN), expires: iso(now0 + 13 * MIN) },
    { ip: '91.240.118.172', reason: 'trap', created: iso(now0 - 3 * 60 * MIN), expires: iso(now0 + 21 * 60 * MIN) },
    { ip: '185.220.101.4', reason: 'trap', created: iso(now0 - 40 * MIN), expires: iso(now0 + 23 * 60 * MIN) },
    { ip: '45.155.205.233', reason: 'ua', created: iso(now0 - 8 * MIN), expires: iso(now0 + 7 * MIN) },
  ],
}

// Seed: sparse traffic over the last 24 h plus a denser last hour (≈ diurnal).
;(function seedLog() {
  const out = []
  for (let t = now0 - 24 * 60 * MIN; t < now0 - 60 * MIN; t += MIN) {
    const hour = new Date(t).getHours()
    const rate = hour >= 1 && hour < 8 ? 0.4 : 1.4
    const n = Math.floor(rate + rnd() * 1.2)
    for (let i = 0; i < n; i++) out.push(makeRequest(t + rnd() * MIN))
  }
  for (let t = now0 - 60 * MIN; t < now0; t += MIN) {
    const spike = t > now0 - 25 * MIN && t < now0 - 18 * MIN ? 18 : 0
    const n = Math.floor(14 + rnd() * 16 + spike)
    for (let i = 0; i < n; i++) out.push(makeRequest(t + rnd() * MIN))
  }
  out.sort((a, b) => a.time.localeCompare(b.time))
  state.log = out.slice(-5000)
})()

/** Append live traffic since the last call (≈ 0.4 req/s). */
function tick(historySize = 5000) {
  const now = Date.now()
  const n = Math.min(400, Math.floor(((now - state.lastTick) / 1000) * (0.3 + rnd() * 0.2)))
  if (n > 0) {
    for (let i = 0; i < n; i++) state.log.push(makeRequest(state.lastTick + ((now - state.lastTick) * (i + 1)) / (n + 1)))
    state.lastTick = now
  }
  if (state.log.length > historySize) state.log.splice(0, state.log.length - historySize)
  const alive = (e) => !e.expires || new Date(e.expires).getTime() > now
  state.blacklist = state.blacklist.filter(alive)
  state.whitelist = state.whitelist.filter(alive)
  state.bans = state.bans.filter(alive)
}

function matches(list, ip) {
  return list.some((e) => coversIp(e.value, ip))
}

function ipState(ip) {
  if (matches(state.whitelist, ip)) return { state: 'whitelisted', banExpires: null }
  if (matches(state.blacklist, ip)) return { state: 'blacklisted', banExpires: null }
  const ban = state.bans.find((b) => b.ip === ip)
  if (ban) return { state: 'banned', banExpires: ban.expires }
  return { state: 'normal', banExpires: null }
}

function statusBucket(s) {
  return `${Math.floor(Number(s) / 100)}xx`
}

function overview(win, enabled) {
  const now = Date.now()
  const span = win === '24h' ? 24 * 60 * MIN : 60 * MIN
  const step = win === '24h' ? 10 * MIN : MIN
  const from = now - span
  const rows = state.log.filter((r) => new Date(r.time).getTime() >= from)
  const buckets = Math.round(span / step)
  const start = Math.floor(from / step) * step + step
  const timeline = Array.from({ length: buckets }, (_, i) => ({ t: iso(start + i * step), requests: 0, blocked: 0 }))
  const statusCodes = { '2xx': 0, '3xx': 0, '4xx': 0, '5xx': 0 }
  const blockedByReason = { blacklist: 0, ban: 0, ua: 0, trap: 0, rate: 0 }
  const ips = new Map()
  const paths = new Map()
  let blocked = 0
  for (const r of rows) {
    const t = new Date(r.time).getTime()
    const idx = Math.min(buckets - 1, Math.max(0, Math.floor((t - start) / step) + 1))
    timeline[idx].requests++
    const b = statusBucket(r.status)
    if (b in statusCodes) statusCodes[b]++
    if (r.blocked) {
      blocked++
      timeline[idx].blocked++
      blockedByReason[r.blocked] = (blockedByReason[r.blocked] || 0) + 1
    }
    const ip = ips.get(r.ip) || { ip: r.ip, requests: 0, blocked: 0, lastSeen: r.time }
    ip.requests++
    if (r.blocked) ip.blocked++
    ip.lastSeen = r.time
    ips.set(r.ip, ip)
    const p = paths.get(r.path) || { path: r.path, requests: 0, errors: 0 }
    p.requests++
    if (r.status >= 400) p.errors++
    paths.set(r.path, p)
  }
  return {
    enabled,
    since: state.since,
    totals: { requests: rows.length, blocked, uniqueIps: ips.size, rps: Math.round((rows.length / (span / 1000)) * 100) / 100 },
    statusCodes,
    blockedByReason,
    timeline,
    topIps: [...ips.values()].sort((a, b) => b.requests - a.requests).slice(0, 10),
    topPaths: [...paths.values()].sort((a, b) => b.requests - a.requests).slice(0, 10),
  }
}

function requests(query) {
  const ip = query.get('ip') || ''
  const path = (query.get('path') || '').toLowerCase()
  const status = (query.get('status') || '').toLowerCase()
  const blocked = query.get('blocked') || ''
  const limit = Math.min(5000, Math.max(1, Number(query.get('limit')) || 200))
  const out = []
  for (let i = state.log.length - 1; i >= 0 && out.length < limit; i--) {
    const r = state.log[i]
    if (ip && r.ip !== ip) continue
    if (path && !r.path.toLowerCase().includes(path)) continue
    if (status && (/^\dxx$/.test(status) ? statusBucket(r.status) !== status : String(r.status) !== status)) continue
    if (blocked && ['1', 'true', 'yes'].includes(blocked) && !r.blocked) continue
    if (blocked && ['0', 'false', 'no'].includes(blocked) && r.blocked) continue
    if (blocked && !['1', 'true', 'yes', '0', 'false', 'no'].includes(blocked) && r.blocked !== blocked) continue
    out.push(r)
  }
  return out
}

function ipsList(query) {
  const sort = query.get('sort') || 'requests'
  const limit = Math.min(1000, Math.max(1, Number(query.get('limit')) || 200))
  const map = new Map()
  for (const r of state.log) {
    const e = map.get(r.ip) || { ip: r.ip, requests: 0, blocked: 0, errors: 0, firstSeen: r.time, lastSeen: r.time, lastPath: r.path, ua: r.ua }
    e.requests++
    if (r.blocked) e.blocked++
    if (r.status >= 400) e.errors++
    e.lastSeen = r.time
    e.lastPath = r.path
    e.ua = r.ua
    map.set(r.ip, e)
  }
  const key = sort === 'lastSeen' ? (e) => e.lastSeen : sort === 'blocked' ? (e) => e.blocked : (e) => e.requests
  return [...map.values()]
    .map((e) => ({ ...e, ...ipState(e.ip) }))
    .sort((a, b) => (key(a) < key(b) ? 1 : key(a) > key(b) ? -1 : 0))
    .slice(0, limit)
}

function selfError(value) {
  const v = String(value).trim()
  if (/^127\./.test(v) || v === '::1' || coversIp(v, '127.0.0.1')) return 'cannot block loopback'
  if (coversIp(v, YOU)) return `refusing to block your own IP (${YOU})`
  return null
}

/**
 * Handle `waf/*`. Returns `[status, payload]` or null when the path is not a
 * WAF endpoint.
 */
export async function handleWaf({ method, path, query, readBody, config }) {
  if (!path.startsWith('waf/')) return null
  const cfg = config.waf || {}
  if (cfg.logRequests !== false) tick(Number(cfg.historySize) || 5000)
  const sub = path.slice(4)

  if (sub === 'overview' && method === 'GET') {
    return [200, { ...overview(query.get('window') === '24h' ? '24h' : '60m', cfg.enable !== false), logRequests: cfg.logRequests !== false }]
  }
  if (sub === 'requests' && method === 'GET') return [200, requests(query)]
  if (sub === 'ips' && method === 'GET') return [200, ipsList(query)]
  if (sub === 'rules' && method === 'GET') {
    return [200, { blacklist: state.blacklist, whitelist: state.whitelist, bans: state.bans, config: cfg, you: YOU }]
  }
  if (sub === 'rules' && method === 'POST') {
    const body = (await readBody()) || {}
    const list = body.list
    const value = String(body.value || '').trim()
    if (list !== 'blacklist' && list !== 'whitelist') return [400, { ok: false, error: 'list must be blacklist or whitelist' }]
    if (!isIpOrCidr(value)) return [400, { ok: false, error: `invalid IP or CIDR: ${value}` }]
    if (list === 'blacklist') {
      const err = selfError(value)
      if (err) return [400, { ok: false, error: err }]
    }
    const minutes = Number(body.expiresMinutes)
    const entry = { value, comment: body.comment || '', created: iso(Date.now()), expires: minutes > 0 ? iso(Date.now() + minutes * MIN) : null }
    state[list] = [...state[list].filter((e) => e.value !== value), entry]
    return [200, { ok: true }]
  }
  if (sub === 'rules' && method === 'DELETE') {
    const list = query.get('list')
    const value = query.get('value')
    if (list !== 'blacklist' && list !== 'whitelist') return [400, { ok: false, error: 'list must be blacklist or whitelist' }]
    if (!state[list].some((e) => e.value === value)) return [404, { ok: false, error: `not found: ${value}` }]
    state[list] = state[list].filter((e) => e.value !== value)
    return [200, { ok: true }]
  }
  if (sub === 'ban' && method === 'POST') {
    const body = (await readBody()) || {}
    const ip = String(body.ip || '').trim()
    const minutes = Number(body.minutes)
    if (!isIpOrCidr(ip) || ip.includes('/')) return [400, { ok: false, error: `invalid IP: ${ip}` }]
    if (!(minutes > 0)) return [400, { ok: false, error: 'minutes must be > 0' }]
    const err = selfError(ip)
    if (err) return [400, { ok: false, error: err }]
    state.bans = [...state.bans.filter((b) => b.ip !== ip), { ip, reason: body.reason || 'manual', created: iso(Date.now()), expires: iso(Date.now() + minutes * MIN) }]
    return [200, { ok: true }]
  }
  if (sub === 'ban' && method === 'DELETE') {
    const ip = query.get('ip')
    if (!state.bans.some((b) => b.ip === ip)) return [404, { ok: false, error: `no active ban for ${ip}` }]
    state.bans = state.bans.filter((b) => b.ip !== ip)
    return [200, { ok: true }]
  }
  if (sub === 'reset' && method === 'POST') {
    state.log = []
    state.since = iso(Date.now())
    state.lastTick = Date.now()
    return [200, { ok: true }]
  }
  return [404, { ok: false, error: 'not found' }]
}
