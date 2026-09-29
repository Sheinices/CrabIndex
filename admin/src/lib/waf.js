// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { tGlobal as tg } from '../lang/index.jsx'

/** Pure WAF helpers: IP/CIDR, domain and bot rule validation, query builders, labels. */

export function isIPv4(s) {
  const parts = String(s).split('.')
  if (parts.length !== 4) return false
  return parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255 && (p === '0' || !p.startsWith('0')))
}

export function isIPv6(s) {
  const v = String(s)
  if (!v.includes(':') || !/^[0-9a-fA-F:.]+$/.test(v)) return false
  // An embedded IPv4 tail is allowed only as the very last component.
  if (v.includes('.') && v.lastIndexOf(':') > v.indexOf('.')) return false
  const halves = v.split('::')
  if (halves.length > 2) return false
  const head = halves[0] ? halves[0].split(':') : []
  const tail = halves.length === 2 && halves[1] ? halves[1].split(':') : []
  const all = [...head, ...tail]
  let groups = 0
  for (let i = 0; i < all.length; i++) {
    const p = all[i]
    if (i === all.length - 1 && p.includes('.')) {
      if (!isIPv4(p)) return false
      groups += 2
    } else if (/^[0-9a-fA-F]{1,4}$/.test(p)) {
      groups++
    } else {
      return false
    }
  }
  return halves.length === 2 ? groups < 8 : groups === 8
}

export function isIp(s) {
  return isIPv4(s) || isIPv6(s)
}

/** IPv4/IPv6 address or CIDR (`/0-32` for v4, `/0-128` for v6). */
export function isIpOrCidr(value) {
  const s = String(value ?? '').trim()
  if (!s) return false
  const slash = s.indexOf('/')
  if (slash === -1) return isIp(s)
  const addr = s.slice(0, slash)
  const bits = s.slice(slash + 1)
  if (!/^\d{1,3}$/.test(bits)) return false
  const n = Number(bits)
  if (isIPv4(addr)) return n <= 32
  if (isIPv6(addr)) return n <= 128
  return false
}

/** Russian validation message for the rule form, or null when valid. */
export function validateRuleValue(value) {
  const s = String(value ?? '').trim()
  if (!s) return tg('waf_v_ip_required')
  if (/\s/.test(s)) return tg('waf_v_ip_spaces')
  if (!isIpOrCidr(s)) return tg('waf_v_ip_invalid')
  return null
}

function ipv4ToInt(ip) {
  return ip.split('.').reduce((acc, p) => acc * 256 + Number(p), 0)
}

function isLoopback(value) {
  const s = String(value).trim().toLowerCase()
  const addr = s.split('/')[0]
  return (isIPv4(addr) && addr.startsWith('127.')) || addr === '::1' || addr === '0:0:0:0:0:0:0:1'
}

/** True if `value` (address or CIDR) covers the IP `you` (IPv4 CIDR aware). */
export function coversIp(value, you) {
  const s = String(value ?? '').trim().toLowerCase()
  const me = String(you ?? '').trim().toLowerCase()
  if (!s || !me) return false
  if (s === me) return true
  const [addr, bits] = s.split('/')
  if (bits === undefined) return false
  if (isIPv4(addr) && isIPv4(me)) {
    const block = 2 ** (32 - Math.min(32, Number(bits)))
    return Math.floor(ipv4ToInt(addr) / block) === Math.floor(ipv4ToInt(me) / block)
  }
  return false
}

/**
 * Client-side self-protection check for blacklist entries / bans. The server
 * enforces the same rule; this only saves a round trip with a clear message.
 */
export function selfBlockError(value, you) {
  if (isLoopback(value)) return tg('waf_v_loopback')
  if (coversIp(value, you)) return tg('waf_v_self', { you })
  return null
}

// --- domains -----------------------------------------------------------------

export const DOMAIN_LISTS = ['domainBlacklist', 'domainWhitelist']

export const isDomainList = (list) => DOMAIN_LISTS.includes(list)

/**
 * Canonical form of a pasted domain, mirroring the server: lowercase, scheme,
 * userinfo, port, path and trailing dot stripped, leading `*.` removed.
 */
export function normalizeDomain(value) {
  let s = String(value ?? '').trim()
  const scheme = s.indexOf('://')
  if (scheme !== -1) s = s.slice(scheme + 3)
  else if (s.startsWith('//')) s = s.slice(2)
  s = s.split(/[/?#\\]/)[0]
  if (s.includes('@')) s = s.slice(s.lastIndexOf('@') + 1)
  s = s.split(':')[0].trim().replace(/\.+$/, '').toLowerCase()
  while (s.startsWith('*.')) s = s.slice(2)
  return s.replace(/^\.+/, '')
}

const DOMAIN_LABEL = /^[\p{L}\p{N}](?:[\p{L}\p{N}-]*[\p{L}\p{N}])?$/u

/** Russian validation message for a domain rule, or null when valid. */
export function validateDomainValue(value) {
  const raw = String(value ?? '').trim()
  if (!raw) return tg('waf_v_domain_required')
  if (/\s/.test(raw)) return tg('waf_v_domain_spaces')
  const d = normalizeDomain(raw)
  if (!d || !d.includes('.') || [...d].length > 253 || !d.split('.').every((l) => [...l].length <= 63 && DOMAIN_LABEL.test(l))) {
    return tg('waf_v_domain_invalid')
  }
  return null
}

/** `host` is `rule` or its subdomain (label boundary). */
export function domainMatches(host, rule) {
  const h = String(host ?? '').toLowerCase()
  const r = String(rule ?? '').toLowerCase()
  return !!r && (h === r || h.endsWith(`.${r}`))
}

/** Builtin entry covering `domain`, or null. */
export function builtinDomainOf(domain, builtins = []) {
  return builtins.find((b) => domainMatches(domain, b)) || null
}

/** Client-side check of a domain rule (format + builtin conflicts), or null. */
export function domainRuleError(list, value, builtins = []) {
  const invalid = validateDomainValue(value)
  if (invalid) return invalid
  const d = normalizeDomain(value)
  const b = builtinDomainOf(d, builtins)
  if (!b) return null
  if (list === 'domainWhitelist') return tg('waf_v_builtin_whitelist', { d })
  return tg('waf_v_builtin_blacklist', { d })
}

/** Query object for `waf/requests` built from the log filters (blanks dropped). */
export function buildRequestsQuery({ ip = '', path = '', origin = '', host = '', status = '', blocked = false, limit = 200 } = {}) {
  const q = {}
  if (ip.trim()) q.ip = ip.trim()
  if (path.trim()) q.path = path.trim()
  if (origin.trim()) q.origin = origin.trim().toLowerCase()
  if (host.trim()) q.host = host.trim().toLowerCase()
  const st = String(status).trim().toLowerCase()
  if (/^[1-5]xx$/.test(st) || /^\d{3}$/.test(st)) q.status = st
  if (blocked) q.blocked = 'true'
  if (limit) q.limit = limit
  return q
}

export const BLOCK_REASONS = {
  blacklist: { get label() { return tg('waf_reason_blacklist') }, tone: 'danger' },
  ban: { get label() { return tg('waf_reason_ban') }, tone: 'danger' },
  ua: { label: 'User-Agent', tone: 'warn' },
  trap: { get label() { return tg('waf_reason_trap') }, tone: 'warn' },
  rate: { get label() { return tg('waf_reason_rate') }, tone: 'warn' },
  domain: { get label() { return tg('waf_reason_domain') }, tone: 'danger' },
  bot: { get label() { return tg('waf_reason_bot') }, tone: 'warn' },
  manual: { get label() { return tg('waf_reason_manual') }, tone: 'danger' },
}

export function reasonLabel(reason) {
  return BLOCK_REASONS[reason]?.label || reason || '-'
}

export function statusTone(status) {
  const s = Number(status)
  if (s >= 500) return 'danger'
  if (s === 403 || s === 429) return 'danger'
  if (s >= 400) return 'warn'
  if (s >= 300) return 'muted'
  if (s >= 200) return 'ok'
  return 'muted'
}

export const TONE_BADGE = {
  ok: 'border-ok/40 bg-ok/10 text-ok',
  warn: 'border-warn/40 bg-warn/10 text-warn',
  danger: 'border-danger/40 bg-danger/10 text-danger',
  muted: 'text-muted',
  brand: 'border-brand/40 bg-brand/10 text-accent',
}

export const IP_STATES = {
  normal: { get label() { return tg('waf_state_normal') }, tone: 'muted' },
  whitelisted: { get label() { return tg('waf_state_whitelisted') }, tone: 'ok' },
  blacklisted: { get label() { return tg('waf_state_blacklisted') }, tone: 'danger' },
  banned: { get label() { return tg('waf_state_banned') }, tone: 'warn' },
}

export const BAN_PRESETS = [
  { minutes: 15, get label() { return tg('waf_exp_15m') } },
  { minutes: 60, get label() { return tg('waf_exp_1h') } },
  { minutes: 1440, get label() { return tg('waf_exp_24h') } },
]

export function formatTime(value) {
  if (!value) return '-'
  const d = new Date(value)
  if (Number.isNaN(d.getTime())) return String(value)
  return d.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

export function percent(part, total) {
  const p = Number(part)
  const t = Number(total)
  if (!(t > 0) || !Number.isFinite(p)) return 0
  return Math.round((p / t) * 1000) / 10
}

// --- bots --------------------------------------------------------------------

export const BOT_LISTS = ['botBlocked', 'botAllowed']

export const isBotList = (list) => BOT_LISTS.includes(list)

export const BOT_RULE_MIN = 3
export const BOT_RULE_MAX = 128

/** Russian validation message for a bot rule (catalog name or User-Agent substring), or null. */
export function validateBotRule(value) {
  const s = String(value ?? '').trim()
  if (!s) return tg('waf_v_bot_required')
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(s)) return tg('waf_v_bot_chars')
  if ([...s].length < BOT_RULE_MIN) return tg('waf_v_bot_short', { n: BOT_RULE_MIN })
  if ([...s].length > BOT_RULE_MAX) return tg('waf_v_bot_long', { n: BOT_RULE_MAX })
  return null
}

/** Warning shown before blocking a whole category (null when blocking is harmless). */
export const BOT_CATEGORY_WARNINGS = {
  get search() { return tg('waf_botcat_warn_search') },
  get libraries() { return tg('waf_botcat_warn_libraries') },
  get empty() { return tg('waf_botcat_warn_empty') },
  get monitoring() { return tg('waf_botcat_warn_monitoring') },
}

export function botCategoryWarning(id) {
  return BOT_CATEGORY_WARNINGS[id] || null
}

export const BOT_STATUS = {
  blocked: { get label() { return tg('waf_bot_state_blocked') }, tone: 'danger' },
  allowed: { get label() { return tg('waf_bot_state_allowed') }, tone: 'ok' },
  seen: { get label() { return tg('waf_bot_state_seen') }, tone: 'muted' },
}

/** Explicit rule for a bot name (`botBlocked` / `botAllowed`, case-insensitive), or null. */
export function botRuleOf(name, rules = {}) {
  const n = String(name ?? '').toLowerCase()
  for (const list of BOT_LISTS) {
    const e = (Array.isArray(rules[list]) ? rules[list] : []).find((r) => String(r.value).toLowerCase() === n)
    if (e) return { list, entry: e }
  }
  return null
}
