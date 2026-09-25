import { describe, expect, it } from 'vitest'
import { normalizeBase, resolveBase } from './base.js'

function docWithMeta(content) {
  const doc = document.implementation.createHTMLDocument('t')
  if (content != null) {
    const m = doc.createElement('meta')
    m.setAttribute('name', 'crab-admin-base')
    m.setAttribute('content', content)
    doc.head.appendChild(m)
  }
  return doc
}

describe('base path', () => {
  it('normalises to a single leading-slash segment', () => {
    expect(normalizeBase('/admin/')).toBe('/admin')
    expect(normalizeBase('panel')).toBe('/panel')
    expect(normalizeBase('/x/settings')).toBe('/x')
    expect(normalizeBase('')).toBe('')
  })

  it('prefers the injected meta tag', () => {
    expect(resolveBase(docWithMeta('/ctrl'), { pathname: '/other/logs' }, false)).toBe('/ctrl')
  })

  it('falls back to the first path segment in production', () => {
    expect(resolveBase(docWithMeta(null), { pathname: '/my-admin/settings' }, false)).toBe('/my-admin')
  })

  it('falls back to /admin in dev', () => {
    expect(resolveBase(docWithMeta(null), { pathname: '/whatever' }, true)).toBe('/admin')
  })
})
