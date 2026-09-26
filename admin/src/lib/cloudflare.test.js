import { describe, expect, it } from 'vitest'
import { advice, avgSeconds, failRate, hostTone, kindLabel, totals } from './cloudflare.js'

const host = (over = {}) => ({
  host: 'example.org',
  browserRequests: 10,
  browserOk: 8,
  browserFailed: 2,
  tabCrashed: 0,
  browserTimeouts: 0,
  challengeFailed: 0,
  fastOk: 40,
  fastFailed: 1,
  sessionsCreated: 1,
  totalBrowserMs: 25_000,
  ...over,
})

describe('cloudflare helpers', () => {
  it('labels known and unknown kinds', () => {
    expect(kindLabel('tabCrashed')).toBe('Вкладка упала')
    expect(kindLabel('weird')).toBe('weird')
  })

  it('computes averages and rates', () => {
    expect(avgSeconds(host())).toBe(2.5)
    expect(failRate(host())).toBe(20)
    expect(avgSeconds(host({ browserRequests: 0 }))).toBeNull()
    expect(failRate({})).toBeNull()
  })

  it('sums totals', () => {
    const t = totals([host(), host({ tabCrashed: 3, browserFailed: 5 })])
    expect(t.browserRequests).toBe(20)
    expect(t.browserFailed).toBe(7)
    expect(t.tabCrashed).toBe(3)
  })

  it('picks a tone per host', () => {
    expect(hostTone(host({ browserRequests: 0 }))).toBe('idle')
    expect(hostTone(host({ browserFailed: 0 }))).toBe('ok')
    expect(hostTone(host())).toBe('warn')
    expect(hostTone(host({ tabCrashed: 1 }))).toBe('bad')
  })

  it('advises raising memory on tab crashes and flags an unreachable solver', () => {
    const a = advice({
      enabled: true,
      paused: false,
      settings: { url: 'http://127.0.0.1:8191/v1' },
      solver: { reachable: false },
      stats: { hosts: [host({ tabCrashed: 4 })] },
    })
    expect(a.map((x) => x.tone)).toEqual(['danger', 'danger'])
    expect(a[1].command).toContain('docker update --memory')
  })

  it('stays quiet when everything is fine', () => {
    expect(advice({ enabled: true, solver: { reachable: true }, stats: { hosts: [host({ browserFailed: 0 })] } })).toEqual([])
    expect(advice(null)).toEqual([])
  })
})
