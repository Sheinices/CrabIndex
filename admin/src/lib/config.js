/** Config editor helpers: path access, sensitive masking, diffs, admin-access changes. */

export const SENSITIVE_FIELD_NAMES = ['apikey', 'devkey', 'cookie', 'u', 'p', 'username', 'password', 'token']
export const MASK = '••••••'

const UNSAFE_KEYS = new Set(['__proto__', 'constructor', 'prototype'])

export function isSafeKey(key) {
  return !!key && !UNSAFE_KEYS.has(key)
}

export function deepClone(obj) {
  return JSON.parse(JSON.stringify(obj ?? {}))
}

export function getByPath(obj, path) {
  if (!obj || !path) return undefined
  let cur = obj
  for (const p of String(path).split('.')) {
    if (!isSafeKey(p) || cur == null || typeof cur !== 'object') return undefined
    cur = cur[p]
  }
  return cur
}

/** Immutable set: returns a new object with `path` set to `value`. */
export function setByPath(obj, path, value) {
  const parts = String(path).split('.')
  if (!parts.every(isSafeKey)) return obj
  const root = Array.isArray(obj) ? [...obj] : { ...(obj || {}) }
  let cur = root
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i]
    const next = cur[p]
    cur[p] = next && typeof next === 'object' && !Array.isArray(next) ? { ...next } : {}
    cur = cur[p]
  }
  cur[parts[parts.length - 1]] = value
  return root
}

export function isSensitiveKey(path, extra = []) {
  const last = String(path || '').split('.').pop().toLowerCase()
  return SENSITIVE_FIELD_NAMES.includes(last) || extra.map((s) => String(s).toLowerCase()).includes(last)
}

function isEmpty(v) {
  return v === undefined || v === null || v === ''
}

/** Display a config value; sensitive values are masked (empty stays visibly empty). */
export function formatValue(value, sensitive = false) {
  if (value === undefined) return '-'
  if (value === null) return 'null'
  if (sensitive) return isEmpty(value) ? '(пусто)' : MASK
  if (typeof value === 'string') return value === '' ? '""' : value
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}

/** Normalise a server diff entry for display, masking secrets. */
export function maskDiffEntry(entry, sensitiveFields = SENSITIVE_FIELD_NAMES) {
  const path = entry?.path ?? ''
  const sensitive = !!entry?.sensitive || isSensitiveKey(path, sensitiveFields)
  return {
    path,
    change: entry?.change || classifyChange(entry?.oldValue, entry?.newValue),
    sensitive,
    oldText: formatValue(entry?.oldValue, sensitive),
    newText: formatValue(entry?.newValue, sensitive),
  }
}

function classifyChange(oldValue, newValue) {
  if (oldValue === undefined || oldValue === null) return 'added'
  if (newValue === undefined || newValue === null) return 'removed'
  return 'modified'
}

function isPlainObject(v) {
  return v != null && typeof v === 'object' && !Array.isArray(v)
}

/**
 * Local structural diff (used for admin-access detection and as a fallback
 * when the server diff is unavailable). Arrays compare as a whole.
 */
export function computeDiff(current, proposed, prefix = '') {
  const out = []
  if (isPlainObject(current) && isPlainObject(proposed)) {
    const keys = new Set([...Object.keys(current), ...Object.keys(proposed)])
    for (const k of [...keys].sort()) {
      out.push(...computeDiff(current[k], proposed[k], prefix ? `${prefix}.${k}` : k))
    }
    return out
  }
  if (isPlainObject(proposed) && current == null) return computeDiff({}, proposed, prefix)
  if (isPlainObject(current) && proposed == null) return computeDiff(current, {}, prefix)
  const a = current === undefined ? null : current
  const b = proposed === undefined ? null : proposed
  if (JSON.stringify(a) === JSON.stringify(b)) return out
  out.push({
    path: prefix,
    oldValue: a,
    newValue: b,
    sensitive: isSensitiveKey(prefix),
    change: classifyChange(a, b),
  })
  return out
}

/** Keys whose change affects how the admin panel is reached or logged into. */
export const ADMIN_ACCESS_KEYS = ['admin.enable', 'admin.path', 'admin.token', 'devkey']

export function adminAccessChanges(current, proposed) {
  return ADMIN_ACCESS_KEYS.filter((k) => {
    const a = getByPath(current, k)
    const b = getByPath(proposed, k)
    return JSON.stringify(a ?? null) !== JSON.stringify(b ?? null)
  })
}

export function normalizeAdminPath(p) {
  const seg = String(p ?? '').trim().replace(/^\/+/, '').replace(/\/+$/, '')
  return seg ? `/${seg}` : '/admin'
}

/** Entry URL `{origin}{path}?{token}` for the given config data. */
export function adminEntryUrl(data, origin = globalThis.location?.origin || '') {
  const path = normalizeAdminPath(getByPath(data, 'admin.path'))
  const token = String(getByPath(data, 'admin.token') ?? '').trim()
  return token ? `${origin}${path}?${token}` : `${origin}${path}`
}

export const ADMIN_PATH_RE = /^\/?[a-z0-9_-]{2,32}\/?$/
export const ADMIN_TOKEN_RE = /^[A-Za-z0-9]{18}$/
export const RESERVED_ADMIN_PATHS = [
  '/', '/api', '/cron', '/dev', '/docs', '/swagger', '/sync', '/stats', '/torznab', '/health', '/version',
  '/lastupdatedb', '/jsondb', '/img', '/assets', '/openapi.yaml', '/opensearch.xml', '/sw.js',
  '/manifest.webmanifest', '/search',
]

/** Client-side pre-check of admin.* values; returns a list of error strings. */
export function validateAdminSection(data) {
  const errors = []
  const admin = getByPath(data, 'admin')
  if (!admin || typeof admin !== 'object') return errors
  if (admin.path != null && admin.path !== '') {
    const p = String(admin.path)
    if (!ADMIN_PATH_RE.test(p)) errors.push('admin.path: один сегмент [a-z0-9_-], 2-32 символа')
    else if (RESERVED_ADMIN_PATHS.includes(normalizeAdminPath(p))) errors.push(`admin.path: путь ${normalizeAdminPath(p)} зарезервирован`)
  }
  if (admin.token != null && admin.token !== '' && !ADMIN_TOKEN_RE.test(String(admin.token))) {
    errors.push('admin.token: ровно 18 символов [A-Za-z0-9]')
  }
  return errors
}

/** Fallback admin group when the server schema does not describe `admin.*`. */
export const ADMIN_GROUP = {
  id: 'admin',
  title: 'Админ-панель',
  description: 'Путь и токен входа в панель. Изменение меняет адрес входа.',
  fields: [
    { key: 'admin.enable', type: 'bool', label: 'Включена', sensitive: false },
    { key: 'admin.path', type: 'string', label: 'Путь', description: 'Один сегмент, например /admin', sensitive: false },
    { key: 'admin.token', type: 'password', label: 'Токен входа', description: '18 символов [A-Za-z0-9]', sensitive: true },
    { key: 'admin.sessionHours', type: 'int', label: 'Сессия (часов)', min: 1, sensitive: false },
  ],
}

export function withAdminGroup(schema) {
  const groups = schema?.groups || []
  const hasAdmin = groups.some(
    (g) => g.id === 'admin' || (g.fields || []).some((f) => String(f.key).startsWith('admin.')),
  )
  if (hasAdmin) return schema
  const idx = groups.findIndex((g) => g.id === 'server')
  const next = [...groups]
  next.splice(idx >= 0 ? idx + 1 : 0, 0, ADMIN_GROUP)
  return { ...schema, groups: next }
}

export function fieldPath(group, tracker, field) {
  return tracker ? `${tracker.id}.${field.key}` : field.key
}

export function stringListToText(val) {
  return Array.isArray(val) ? val.join('\n') : ''
}

export function textToStringList(text) {
  return String(text || '')
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean)
}

/** Random admin token matching `[A-Za-z0-9]{18}`. */
export function generateToken(len = 18) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  const bytes = new Uint8Array(len)
  globalThis.crypto.getRandomValues(bytes)
  return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('')
}
