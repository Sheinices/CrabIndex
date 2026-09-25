import { describe, expect, it } from 'vitest'
import {
  adminAccessChanges,
  adminEntryUrl,
  computeDiff,
  formatValue,
  getByPath,
  isSensitiveKey,
  MASK,
  maskDiffEntry,
  setByPath,
  validateAdminSection,
  withAdminGroup,
  generateToken,
  ADMIN_TOKEN_RE,
} from './config.js'

describe('path helpers', () => {
  it('reads and immutably writes dotted paths', () => {
    const src = { Rutor: { login: { u: 'a' } } }
    const next = setByPath(src, 'Rutor.login.p', 'secret')
    expect(getByPath(next, 'Rutor.login.p')).toBe('secret')
    expect(getByPath(next, 'Rutor.login.u')).toBe('a')
    expect(src.Rutor.login.p).toBeUndefined()
    expect(next).not.toBe(src)
  })

  it('refuses prototype-polluting keys', () => {
    const next = setByPath({}, '__proto__.polluted', 1)
    expect({}.polluted).toBeUndefined()
    expect(next).toEqual({})
    expect(getByPath({ a: 1 }, 'constructor.name')).toBeUndefined()
  })
})

describe('masking', () => {
  it('detects sensitive keys by last segment', () => {
    expect(isSensitiveKey('devkey')).toBe(true)
    expect(isSensitiveKey('Kinozal.login.p')).toBe(true)
    expect(isSensitiveKey('admin.token')).toBe(true)
    expect(isSensitiveKey('Kinozal.host')).toBe(false)
    expect(isSensitiveKey('x.custom', ['custom'])).toBe(true)
  })

  it('masks sensitive values but shows emptiness', () => {
    expect(formatValue('hunter2', true)).toBe(MASK)
    expect(formatValue(null, true)).toBe('null')
    expect(formatValue('', true)).toBe('(пусто)')
    expect(formatValue({ a: 1 })).toBe('{"a":1}')
  })

  it('masks diff entries flagged by the server or by key name', () => {
    const byServer = maskDiffEntry({ path: 'alloha.x', oldValue: 'a', newValue: 'b', sensitive: true, change: 'modified' })
    expect(byServer).toMatchObject({ oldText: MASK, newText: MASK, sensitive: true })
    const byName = maskDiffEntry({ path: 'Rutor.cookie', oldValue: null, newValue: 'uid=1' })
    expect(byName).toMatchObject({ oldText: 'null', newText: MASK, change: 'added' })
    const plain = maskDiffEntry({ path: 'listenport', oldValue: '9117', newValue: '9118', sensitive: false, change: 'modified' })
    expect(plain).toMatchObject({ oldText: '9117', newText: '9118', sensitive: false })
  })
})

describe('computeDiff', () => {
  it('lists changed leaf keys with change kinds', () => {
    const d = computeDiff({ a: 1, b: { c: 'x', d: [1] }, gone: true }, { a: 1, b: { c: 'y', d: [1, 2] }, added: 'z' })
    expect(d.map((e) => [e.path, e.change])).toEqual([
      ['added', 'added'],
      ['b.c', 'modified'],
      ['b.d', 'modified'],
      ['gone', 'removed'],
    ])
  })

  it('flags sensitive leaves', () => {
    const d = computeDiff({ devkey: 'a' }, { devkey: 'b' })
    expect(d[0].sensitive).toBe(true)
  })
})

describe('admin access', () => {
  const cur = { devkey: 'k', admin: { enable: true, path: '/admin', token: 'Z0mt0N7r2hoUM2TuOk' } }

  it('detects path/token/devkey changes', () => {
    expect(adminAccessChanges(cur, cur)).toEqual([])
    expect(adminAccessChanges(cur, { ...cur, devkey: 'n' })).toEqual(['devkey'])
    expect(adminAccessChanges(cur, { ...cur, admin: { ...cur.admin, path: '/ctl', token: 'AAAAAAAAAAAAAAAAAA' } })).toEqual(['admin.path', 'admin.token'])
  })

  it('builds the entry URL', () => {
    expect(adminEntryUrl(cur, 'http://h:9117')).toBe('http://h:9117/admin?Z0mt0N7r2hoUM2TuOk')
    expect(adminEntryUrl({ admin: { path: 'ctl/', token: '' } }, 'http://h')).toBe('http://h/ctl')
  })

  it('validates admin.path and admin.token', () => {
    expect(validateAdminSection(cur)).toEqual([])
    expect(validateAdminSection({ admin: { path: '/api' } })[0]).toMatch(/зарезервирован/)
    expect(validateAdminSection({ admin: { path: '/a/b' } })[0]).toMatch(/admin.path/)
    expect(validateAdminSection({ admin: { token: 'short' } })[0]).toMatch(/admin.token/)
  })

  it('generates valid tokens', () => {
    expect(generateToken()).toMatch(ADMIN_TOKEN_RE)
  })

  it('adds an admin group only when the schema lacks one', () => {
    const s = withAdminGroup({ groups: [{ id: 'server', fields: [] }, { id: 'api', fields: [] }] })
    expect(s.groups.map((g) => g.id)).toEqual(['server', 'admin', 'api'])
    const has = { groups: [{ id: 'x', fields: [{ key: 'admin.path' }] }] }
    expect(withAdminGroup(has)).toBe(has)
  })
})
