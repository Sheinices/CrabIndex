/**
 * Admin base path ({admin.path}), e.g. `/admin`.
 *
 * Order: `<meta name="crab-admin-base">` injected by the server → first path
 * segment of `location.pathname` → `/admin` (dev fallback).
 */
export function normalizeBase(value) {
  const raw = String(value ?? '').trim()
  if (!raw) return ''
  const seg = raw.replace(/^\/+/, '').replace(/\/+$/, '').split('/')[0]
  return seg ? `/${seg}` : ''
}

export function resolveBase(doc = globalThis.document, loc = globalThis.location, isDev = import.meta.env?.DEV) {
  const meta = doc?.querySelector?.('meta[name="crab-admin-base"]')?.getAttribute('content')
  const fromMeta = normalizeBase(meta)
  if (fromMeta) return fromMeta
  if (isDev) return '/admin'
  const fromPath = normalizeBase(loc?.pathname)
  return fromPath || '/admin'
}

let cached = null

export function getBase() {
  if (cached == null) cached = resolveBase()
  return cached
}

/** Test helper. */
export function setBaseForTests(value) {
  cached = value
}
