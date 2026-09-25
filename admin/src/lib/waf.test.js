import { describe, expect, it } from 'vitest'
import { buildRequestsQuery, coversIp, isIpOrCidr, selfBlockError, validateRuleValue } from './waf.js'

describe('IP / CIDR validation', () => {
  it.each(['203.0.113.7', '0.0.0.0', '255.255.255.255', '198.51.100.0/24', '10.0.0.0/8', '1.2.3.4/32', '::1', '2001:db8::/32', 'fe80::1', '2001:0db8:85a3:0000:0000:8a2e:0370:7334', '::ffff:192.0.2.1', '::/0', '2001:db8::/128'])(
    'accepts %s',
    (v) => expect(isIpOrCidr(v)).toBe(true),
  )

  it.each(['', '256.1.1.1', '1.2.3', '1.2.3.4.5', '01.2.3.4', '1.2.3.4/33', '1.2.3.4/', '1.2.3.4/-1', 'abc', '2001:db8::/129', '1::2::3', '2001:db8:1:2:3:4:5:6:7', 'gggg::1', '1.2.3.4::', '10.0.0.0/8/8', ' 1.2.3.4 x'])(
    'rejects %s',
    (v) => expect(isIpOrCidr(v)).toBe(false),
  )

  it('produces Russian messages', () => {
    expect(validateRuleValue('')).toMatch(/Укажите/)
    expect(validateRuleValue('1.2.3.4 5.6.7.8')).toMatch(/пробел/)
    expect(validateRuleValue('300.1.1.1')).toMatch(/Некорректный/)
    expect(validateRuleValue(' 198.51.100.0/24 ')).toBeNull()
  })

  it('detects self-blocking (exact, CIDR, loopback)', () => {
    expect(coversIp('192.168.1.0/24', '192.168.1.10')).toBe(true)
    expect(coversIp('192.168.2.0/24', '192.168.1.10')).toBe(false)
    expect(coversIp('0.0.0.0/0', '8.8.8.8')).toBe(true)
    expect(selfBlockError('192.168.1.10', '192.168.1.10')).toMatch(/собственный/)
    expect(selfBlockError('127.0.0.1', '10.0.0.1')).toMatch(/loopback/)
    expect(selfBlockError('::1', null)).toMatch(/loopback/)
    expect(selfBlockError('203.0.113.7', '192.168.1.10')).toBeNull()
  })
})

describe('buildRequestsQuery', () => {
  it('drops blank filters and keeps the limit', () => {
    expect(buildRequestsQuery({ ip: ' ', path: '', status: '', blocked: false })).toEqual({ limit: 200 })
  })

  it('maps every filter', () => {
    expect(buildRequestsQuery({ ip: ' 203.0.113.7 ', path: '/api', status: '4XX', blocked: true, limit: 500 })).toEqual({
      ip: '203.0.113.7',
      path: '/api',
      status: '4xx',
      blocked: 'true',
      limit: 500,
    })
    expect(buildRequestsQuery({ status: '404' }).status).toBe('404')
    expect(buildRequestsQuery({ status: 'bogus' }).status).toBeUndefined()
  })
})
