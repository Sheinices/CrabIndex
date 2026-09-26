/**
 * Dev-only WAF mock (see WAF_CONTRACT.md): a synthetic request log that keeps
 * growing while the dev server runs, dynamic lists/bans and all `waf/*`
 * endpoints including the self-protection error.
 */
import { coversIp, domainRuleError, isIpOrCidr, normalizeDomain, validateBotRule, validateDomainValue } from '../src/lib/waf.js'

const MIN = 60_000
export const YOU = '192.168.1.10'

/** Mirrors the compiled-in server list (crates/crabindex/src/waf/domains.rs). */
export const BUILTIN_DOMAINS = [
  'ndst.pw',
  'diskstation.me',
  'krilzov.it',
  'myds.me',
  'lampa.stream',
  'bylampa.online',
  'abhq.ru',
  'abmsx.tech',
  'akter.black',
  'lampa.click',
  'lampa.land',
  'lampa1.ru',
  'line.pm',
  'nnmtv.pw',
  'tvigl.info',
  'uspeh.sbs',
  'usph.xyz',
  'xabb.ru',
]

const UA = {
  lampa: 'Mozilla/5.0 (Linux; Android 12; SHIELD Android TV) AppleWebKit/537.36 Lampa/2.3.1',
  jackett: 'Jackett/0.22.1880',
  prowlarr: 'Prowlarr/1.24.3.4754 (ubuntu 24.04)',
  curl: 'curl/8.9.1',
  chrome: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36',
  sqlmap: 'sqlmap/1.8.9#stable (https://sqlmap.org)',
  nikto: 'Mozilla/5.00 (Nikto/2.5.0) (Evasions:None) (Test:Port Check)',
  bot: 'Mozilla/5.0 (compatible; zgrab/0.x)',
  googlebot: 'Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)',
  yandex: 'Mozilla/5.0 (compatible; YandexBot/3.0; +http://yandex.com/bots)',
  ahrefs: 'Mozilla/5.0 (compatible; AhrefsBot/7.0; +http://ahrefs.com/robot/)',
  gptbot: 'Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; GPTBot/1.2; +https://openai.com/gptbot)',
  claudebot: 'Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0; +claudebot@anthropic.com)',
  telegram: 'TelegramBot (like TwitterBot)',
  uptime: 'Mozilla/5.0+(compatible; UptimeRobot/2.0; http://www.uptimerobot.com/)',
  pyreq: 'python-requests/2.32.3',
  censys: 'Mozilla/5.0 (compatible; CensysInspect/1.1; +https://about.censys.io/)',
  foobot: 'Mozilla/5.0 (compatible; FooBot/1.0; +https://foo.example/bot)',
  empty: '',
}

/** Server host names clients address (the `Host` header). */
const HOST = { main: 'sync.crab.rip', ip: '198.51.100.1', mirror: 'torrents-mirror.example' }

const API = ['/api/v1.0/torrents', '/api/v2.0/indexers/all/results', '/api/v1.0/torrents', '/api/v2.0/indexers/rutracker/results/torznab', '/lastupdatedb', '/api/v1.0/conf']
const TRAPS = ['/.env', '/wp-login.php', '/.git/config', '/phpmyadmin/index.php', '/wp-admin/setup-config.php']

// weight = share of traffic, kind drives status/blocked outcome.
const CLIENTS = [
  { ip: YOU, ua: UA.chrome, weight: 6, kind: 'admin', host: 'localhost' },
  { ip: '10.0.0.5', ua: UA.curl, weight: 4, kind: 'cron', host: 'localhost' },
  { ip: '198.51.100.23', ua: UA.lampa, weight: 22, kind: 'api', origin: 'my-lampa.example' },
  { ip: '198.51.100.77', ua: UA.jackett, weight: 14, kind: 'api' },
  { ip: '2001:db8:85a3::8a2e:370:7334', ua: UA.prowlarr, weight: 10, kind: 'api' },
  { ip: '93.184.216.34', ua: UA.lampa, weight: 9, kind: 'api', origin: 'lampa.mx' },
  { ip: '176.59.40.12', ua: UA.lampa, weight: 6, kind: 'domain', origin: 'app.ndst.pw' },
  { ip: '46.39.230.8', ua: UA.chrome, weight: 3, kind: 'domain', origin: 'lampa.stream' },
  { ip: '37.145.12.90', ua: UA.chrome, weight: 2, kind: 'domain', origin: 'mirror.spam-tracker.example' },
  { ip: '5.188.62.140', ua: UA.curl, weight: 12, kind: 'flood' },
  { ip: '203.0.113.7', ua: UA.bot, weight: 5, kind: 'blacklisted' },
  { ip: '45.155.205.233', ua: UA.sqlmap, weight: 3, kind: 'ua' },
  { ip: '91.240.118.172', ua: UA.nikto, weight: 3, kind: 'trap' },
  { ip: '185.220.101.4', ua: UA.bot, weight: 2, kind: 'trap' },
  { ip: '192.0.2.10', ua: UA.curl, weight: 4, kind: 'banned' },
  { ip: '66.249.66.1', ua: UA.googlebot, weight: 3, kind: 'crawler' },
  { ip: '5.255.253.10', ua: UA.yandex, weight: 2, kind: 'crawler' },
  { ip: '54.36.148.20', ua: UA.ahrefs, weight: 4, kind: 'crawler' },
  { ip: '54.36.149.33', ua: UA.ahrefs, weight: 2, kind: 'crawler' },
  { ip: '20.171.207.4', ua: UA.gptbot, weight: 3, kind: 'crawler' },
  { ip: '3.224.220.101', ua: UA.claudebot, weight: 2, kind: 'crawler' },
  { ip: '149.154.161.200', ua: UA.telegram, weight: 1, kind: 'crawler' },
  { ip: '69.162.124.230', ua: UA.uptime, weight: 1, kind: 'crawler', paths: ['/health'] },
  { ip: '167.94.138.60', ua: UA.censys, weight: 2, kind: 'crawler', host: HOST.ip, paths: ['/', '/robots.txt'] },
  { ip: '45.83.64.12', ua: UA.pyreq, weight: 2, kind: 'api', host: HOST.mirror },
  { ip: '193.35.18.9', ua: UA.empty, weight: 2, kind: 'crawler', host: HOST.ip, paths: ['/', '/favicon.ico'] },
  { ip: '212.102.40.7', ua: UA.foobot, weight: 1, kind: 'crawler' },
]

/** Small mirror of the server bot catalog (crates/crabindex/src/waf/bots.rs), enough for the mock traffic. */
const BOT_CATALOG = {
  search: ['Googlebot', 'bingbot', 'YandexImages', 'YandexBot', 'Baiduspider', 'DuckDuckBot', 'Applebot', 'Sogou', 'Exabot', 'SeznamBot', 'PetalBot', 'Yahoo Slurp'],
  ai: ['GPTBot', 'ChatGPT-User', 'OAI-SearchBot', 'ClaudeBot', 'Claude-Web', 'anthropic-ai', 'CCBot', 'Bytespider', 'PerplexityBot', 'Google-Extended', 'Amazonbot'],
  seo: ['AhrefsBot', 'SemrushBot', 'MJ12bot', 'DotBot', 'BLEXBot', 'DataForSeoBot', 'serpstatbot', 'Barkrowler', 'SeekportBot', 'MegaIndex', 'Screaming Frog', 'rogerbot'],
  social: ['facebookexternalhit', 'TelegramBot', 'Twitterbot', 'WhatsApp', 'Discordbot', 'Slackbot', 'LinkedInBot', 'vkShare', 'SkypeUriPreview'],
  monitoring: ['UptimeRobot', 'Pingdom', 'StatusCake', 'Better Uptime', 'Site24x7', 'Datadog', 'HetrixTools'],
  scanners: ['zgrab', 'masscan', 'Nmap', 'Nuclei', 'sqlmap', 'Nikto', 'CensysInspect', 'Censys', 'Expanse', 'InternetMeasurement', 'ModatScanner', 'l9explore', 'WPScan'],
  libraries: ['python-requests', 'python-urllib', 'aiohttp', 'Go-http-client', 'curl', 'Wget', 'okhttp', 'Java', 'node-fetch', 'axios', 'libwww-perl', 'PHP', 'Scrapy', 'HeadlessChrome'],
}
const BOT_CATEGORIES = [
  ['search', 'Поисковые роботы', 'Индексируют сайты для поисковых систем (Google, Яндекс, Bing и другие)'],
  ['ai', 'AI-краулеры', 'Собирают данные для обучения и ответов нейросетей'],
  ['seo', 'SEO-сервисы', 'Анализ ссылок и позиций сайтов (Ahrefs, Semrush и другие)'],
  ['social', 'Соцсети и мессенджеры', 'Строят превью ссылок в соцсетях и мессенджерах'],
  ['monitoring', 'Мониторинг доступности', 'Проверяют, что сервер отвечает (UptimeRobot, Pingdom и другие)'],
  ['scanners', 'Сканеры уязвимостей', 'Массовые сканеры интернета и поиск уязвимостей'],
  ['libraries', 'HTTP-библиотеки и утилиты', 'Скрипты и утилиты без собственного имени (curl, python-requests, Go и другие)'],
  ['empty', 'Пустой User-Agent', 'Запросы без заголовка User-Agent'],
  ['other', 'Прочие боты', 'User-Agent со словами bot, crawler, spider, slurp, которых нет в каталоге'],
]
const SIGNATURES = { curl: 'curl/', Wget: 'wget/', Java: 'java/', PHP: 'php/', axios: 'axios/', 'Yahoo Slurp': 'yahoo! slurp' }

/** `{category, name}` of a User-Agent or null (catalog, empty UA, bot|crawler|spider heuristic). */
function classifyBot(ua) {
  const s = String(ua || '').trim()
  if (!s) return { category: 'empty', name: '(пустой)' }
  const lower = s.toLowerCase()
  for (const [category, names] of Object.entries(BOT_CATALOG)) {
    for (const name of names) if (lower.includes(SIGNATURES[name] || name.toLowerCase())) return { category, name }
  }
  const token = s.split(/[\s;(),+]+/).find((t) => !t.includes('://') && /bot|crawl|spider|slurp/i.test(t.split('/')[0]))
  return token ? { category: 'other', name: token.split('/')[0] } : null
}
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
  const base = {
    time: new Date(time).toISOString(),
    ip: c.ip,
    method: 'GET',
    ua: c.ua,
    ms: Math.round(4 + rnd() * 60),
    blocked: null,
    origin: c.origin || null,
    host: c.host || (rnd() < 0.06 ? HOST.ip : HOST.main),
  }
  const out = outcome(c, base)
  // Bot rules apply to requests nothing else refused (LAN / admin are never affected).
  if (!out.blocked && c.kind !== 'admin' && c.kind !== 'cron' && botBlocked(c.ua)) return { ...out, status: 403, blocked: 'bot', ms: 0 }
  return out
}

function outcome(c, base) {
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
    case 'domain':
      return { ...base, path: pick(API), status: 403, blocked: 'domain', ms: 0 }
    case 'banned':
      return { ...base, path: pick(API), status: 403, blocked: 'ban', ms: 0 }
    case 'crawler':
      return { ...base, path: pick(c.paths || ['/', '/robots.txt', '/api/v1.0/torrents', '/docs/', '/sitemap.xml']), status: rnd() < 0.8 ? 200 : 404 }
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
  domainBlacklist: [{ value: 'spam-tracker.example', comment: 'чужой парсер', created: iso(now0 - 2 * 86_400_000), expires: null }],
  domainWhitelist: [{ value: 'my-lampa.example', comment: 'своя Lampa', created: iso(now0 - 5 * 86_400_000), expires: null }],
  botBlockCategories: ['ai'],
  botBlocked: [{ value: 'AhrefsBot', comment: 'грузит /api', created: iso(now0 - 86_400_000), expires: null }],
  botAllowed: [{ value: 'UptimeRobot', comment: 'свой мониторинг', created: iso(now0 - 7 * 86_400_000), expires: null }],
  robotsDisallow: false,
}

const alive = (e) => !e.expires || new Date(e.expires).getTime() > Date.now()
const ruleHits = (value, bot, lowerUa) => {
  const v = String(value).toLowerCase()
  return (bot && bot.name.toLowerCase() === v) || lowerUa.includes(v)
}

/** Mirror of the server decision: `botAllowed` wins, then a blocked category or `botBlocked`. */
function botBlocked(ua) {
  const bot = classifyBot(ua)
  const lower = String(ua || '').toLowerCase()
  if (state.botAllowed.filter(alive).some((e) => ruleHits(e.value, bot, lower))) return false
  return (bot && state.botBlockCategories.includes(bot.category)) || state.botBlocked.filter(alive).some((e) => ruleHits(e.value, bot, lower))
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
  state.botBlocked = state.botBlocked.filter(alive)
  state.botAllowed = state.botAllowed.filter(alive)
  state.blacklist = state.blacklist.filter(alive)
  state.whitelist = state.whitelist.filter(alive)
  state.bans = state.bans.filter(alive)
  state.domainBlacklist = state.domainBlacklist.filter(alive)
  state.domainWhitelist = state.domainWhitelist.filter(alive)
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
  const blockedByReason = { blacklist: 0, ban: 0, ua: 0, trap: 0, rate: 0, domain: 0, bot: 0 }
  const ips = new Map()
  const origins = new Map()
  const hosts = new Map()
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
    if (r.origin) {
      const o = origins.get(r.origin) || { origin: r.origin, requests: 0, blocked: 0 }
      o.requests++
      if (r.blocked) o.blocked++
      origins.set(r.origin, o)
    }
    const hk = r.host || '-'
    const h = hosts.get(hk) || { host: hk, requests: 0, blocked: 0 }
    h.requests++
    if (r.blocked) h.blocked++
    hosts.set(hk, h)
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
    topOrigins: [...origins.values()].sort((a, b) => b.requests - a.requests).slice(0, 20),
    topHosts: [...hosts.values()].sort((a, b) => b.requests - a.requests).slice(0, 20),
  }
}

function requests(query) {
  const ip = query.get('ip') || ''
  const path = (query.get('path') || '').toLowerCase()
  const origin = (query.get('origin') || '').trim().toLowerCase()
  const host = (query.get('host') || '').trim().toLowerCase()
  const status = (query.get('status') || '').toLowerCase()
  const blocked = query.get('blocked') || ''
  const limit = Math.min(5000, Math.max(1, Number(query.get('limit')) || 200))
  const out = []
  for (let i = state.log.length - 1; i >= 0 && out.length < limit; i--) {
    const r = state.log[i]
    if (ip && r.ip !== ip) continue
    if (path && !r.path.toLowerCase().includes(path)) continue
    if (origin && !(r.origin || '').includes(origin)) continue
    if (host && !(r.host || '-').includes(host)) continue
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

/** `GET waf/bots` payload computed from the synthetic log and the bot rules. */
function botsView() {
  const map = new Map()
  const totals = Object.fromEntries(BOT_CATEGORIES.map(([id]) => [id, { requests: 0, blocked: 0 }]))
  for (const r of state.log) {
    const bot = classifyBot(r.ua)
    if (!bot) continue
    totals[bot.category].requests++
    if (r.blocked) totals[bot.category].blocked++
    const key = `${bot.category}:${bot.name}`
    const b = map.get(key) || { ...bot, requests: 0, blocked: 0, lastSeen: r.time, ipSet: new Set(), paths: new Map(), samples: [] }
    b.requests++
    if (r.blocked) b.blocked++
    b.lastSeen = r.time
    b.ipSet.add(r.ip)
    b.paths.set(r.path, (b.paths.get(r.path) || 0) + 1)
    if (r.ua && b.samples.length < 5 && !b.samples.includes(r.ua)) b.samples.push(r.ua)
    map.set(key, b)
  }
  const statusOf = (b) => {
    const hit = (e) => ruleHits(e.value, b, b.samples.join('\n').toLowerCase())
    if (state.botAllowed.filter(alive).some(hit)) return 'allowed'
    if (state.botBlockCategories.includes(b.category) || state.botBlocked.filter(alive).some(hit)) return 'blocked'
    return 'seen'
  }
  const bots = [...map.values()]
    .sort((a, b) => b.requests - a.requests)
    .map(({ ipSet, paths, ...b }) => ({
      ...b,
      ips: ipSet.size,
      topPaths: [...paths.entries()]
        .sort((x, y) => y[1] - x[1])
        .slice(0, 5)
        .map(([path, requests]) => ({ path, requests })),
      status: statusOf(b),
    }))
  return {
    categories: BOT_CATEGORIES.map(([id, label, description]) => ({
      id,
      label,
      description,
      ...totals[id],
      blockedCategory: state.botBlockCategories.includes(id),
      botCount: bots.filter((b) => b.category === id).length,
    })),
    bots,
    rules: { botBlockCategories: state.botBlockCategories, botBlocked: state.botBlocked, botAllowed: state.botAllowed, robotsDisallow: state.robotsDisallow },
    catalog: BOT_CATALOG,
  }
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
    return [
      200,
      {
        blacklist: state.blacklist,
        whitelist: state.whitelist,
        bans: state.bans,
        domainBlacklist: state.domainBlacklist,
        domainWhitelist: state.domainWhitelist,
        builtinDomains: BUILTIN_DOMAINS,
        config: cfg,
        you: YOU,
      },
    ]
  }
  if (sub === 'rules' && method === 'POST') {
    const body = (await readBody()) || {}
    const list = body.list
    const value = String(body.value || '').trim()
    if (list === 'domainBlacklist' || list === 'domainWhitelist') {
      const err = domainRuleError(list, value, BUILTIN_DOMAINS)
      if (err) return [400, { ok: false, error: err }]
      const d = normalizeDomain(value)
      const minutes = Number(body.expiresMinutes)
      const entry = { value: d, comment: body.comment || '', created: iso(Date.now()), expires: minutes > 0 ? iso(Date.now() + minutes * MIN) : null }
      state[list] = [...state[list].filter((e) => e.value !== d), entry]
      return [200, { ok: true }]
    }
    if (list !== 'blacklist' && list !== 'whitelist') return [400, { ok: false, error: 'list: blacklist, whitelist, domainBlacklist or domainWhitelist expected' }]
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
    let value = query.get('value')
    if (list === 'domainBlacklist' || list === 'domainWhitelist') {
      const invalid = validateDomainValue(value)
      if (invalid) return [400, { ok: false, error: invalid }]
      value = normalizeDomain(value)
      if (BUILTIN_DOMAINS.includes(value)) return [400, { ok: false, error: `${value} входит во встроенный список и не может быть удалён` }]
    } else if (list !== 'blacklist' && list !== 'whitelist') {
      return [400, { ok: false, error: 'list: blacklist, whitelist, domainBlacklist or domainWhitelist expected' }]
    }
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
  if (sub === 'bots' && method === 'GET') return [200, botsView()]
  if (sub === 'bots/category' && method === 'POST') {
    const body = (await readBody()) || {}
    const id = String(body.id || '')
    if (!BOT_CATEGORIES.some(([c]) => c === id)) return [400, { ok: false, error: `id: ${BOT_CATEGORIES.map(([c]) => c).join(', ')} expected` }]
    if (typeof body.block !== 'boolean') return [400, { ok: false, error: 'block: true or false expected' }]
    state.botBlockCategories = body.block ? [...new Set([...state.botBlockCategories, id])] : state.botBlockCategories.filter((c) => c !== id)
    return [200, { ok: true }]
  }
  if (sub === 'bots/rules' && method === 'POST') {
    const body = (await readBody()) || {}
    const list = body.list
    if (list !== 'botBlocked' && list !== 'botAllowed') return [400, { ok: false, error: 'list: botBlocked or botAllowed expected' }]
    const value = String(body.value || '').trim()
    const err = validateBotRule(value)
    if (err) return [400, { ok: false, error: `value: ${err}` }]
    const other = list === 'botBlocked' ? 'botAllowed' : 'botBlocked'
    const same = (e) => e.value.toLowerCase() === value.toLowerCase()
    const minutes = Number(body.expiresMinutes)
    const entry = { value, comment: body.comment || '', created: iso(Date.now()), expires: minutes > 0 ? iso(Date.now() + minutes * MIN) : null }
    state[other] = state[other].filter((e) => !same(e))
    state[list] = [...state[list].filter((e) => !same(e)), entry]
    return [200, { ok: true }]
  }
  if (sub === 'bots/rules' && method === 'DELETE') {
    const list = query.get('list')
    const value = String(query.get('value') || '').trim().toLowerCase()
    if (list !== 'botBlocked' && list !== 'botAllowed') return [400, { ok: false, error: 'list: botBlocked or botAllowed expected' }]
    if (!state[list].some((e) => e.value.toLowerCase() === value)) return [404, { ok: false, error: 'not found' }]
    state[list] = state[list].filter((e) => e.value.toLowerCase() !== value)
    return [200, { ok: true }]
  }
  if (sub === 'bots/robots' && method === 'POST') {
    const body = (await readBody()) || {}
    if (typeof body.disallow !== 'boolean') return [400, { ok: false, error: 'disallow: true or false expected' }]
    state.robotsDisallow = body.disallow
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
