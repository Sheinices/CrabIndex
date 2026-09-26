import { describe, expect, it } from 'vitest'
import {
  BLOCK_REASONS,
  botCategoryWarning,
  botRuleOf,
  buildRequestsQuery,
  builtinDomainOf,
  coversIp,
  domainMatches,
  domainRuleError,
  isBotList,
  isIpOrCidr,
  normalizeDomain,
  reasonLabel,
  selfBlockError,
  validateBotRule,
  validateDomainValue,
  validateRuleValue,
} from './waf.js'

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

describe('domain rules', () => {
  const BUILTIN = ['ndst.pw', 'myds.me', 'lampa.stream']

  it.each([
    ['Example.COM', 'example.com'],
    ['*.example.com', 'example.com'],
    ['https://Sub.Example.com:8443/path?q=1#x', 'sub.example.com'],
    ['example.com.', 'example.com'],
    ['//cdn.example.com/x', 'cdn.example.com'],
    ['пример.рф', 'пример.рф'],
  ])('normalises %s', (input, out) => expect(normalizeDomain(input)).toBe(out))

  it.each(['example.com', 'a-b.example.co.uk', '*.example.com', 'https://example.com/x', 'пример.рф', 'xn--e1afmkfd.xn--p1ai'])('accepts %s', (v) =>
    expect(validateDomainValue(v)).toBeNull(),
  )

  it.each(['localhost', 'ex_ample.com', '-a.com', 'a-.com', 'a..com', '*.', `${'a'.repeat(250)}.com`, `${'a'.repeat(64)}.com`])('rejects %s', (v) =>
    expect(validateDomainValue(v)).toMatch(/Некорректный домен/),
  )

  it('needs a value without spaces', () => {
    expect(validateDomainValue(' ')).toMatch(/Укажите домен/)
    expect(validateDomainValue('a.com b.com')).toMatch(/пробел/)
  })

  it('matches on a label boundary', () => {
    expect(domainMatches('a.b.ndst.pw', 'ndst.pw')).toBe(true)
    expect(domainMatches('ndst.pw', 'ndst.pw')).toBe(true)
    expect(domainMatches('notndst.pw', 'ndst.pw')).toBe(false)
    expect(builtinDomainOf('app.myds.me', BUILTIN)).toBe('myds.me')
    expect(builtinDomainOf('mylampa.stream', BUILTIN)).toBeNull()
  })

  it('refuses builtin conflicts', () => {
    expect(domainRuleError('domainWhitelist', 'https://app.NDST.pw', BUILTIN)).toBe('app.ndst.pw заблокирован встроенным списком и не может быть разрешён')
    expect(domainRuleError('domainBlacklist', 'myds.me', BUILTIN)).toMatch(/уже заблокирован встроенным списком/)
    expect(domainRuleError('domainWhitelist', 'friend.example', BUILTIN)).toBeNull()
    expect(domainRuleError('domainBlacklist', 'nodot', BUILTIN)).toMatch(/Некорректный/)
  })

  it('adds the origin filter to the requests query', () => {
    expect(buildRequestsQuery({ origin: ' NDST.pw ' })).toEqual({ origin: 'ndst.pw', limit: 200 })
  })
})

describe('bots and hosts', () => {
  it('adds the host filter to the requests query', () => {
    expect(buildRequestsQuery({ host: ' Sync.Crab.RIP ' })).toEqual({ host: 'sync.crab.rip', limit: 200 })
    expect(buildRequestsQuery({ host: '  ' })).toEqual({ limit: 200 })
  })

  it('labels the bot block reason', () => {
    expect(reasonLabel('bot')).toBe('Бот')
    expect(BLOCK_REASONS.bot.tone).toBe('warn')
  })

  it('validates bot rules', () => {
    expect(validateBotRule('')).toMatch(/Укажите/)
    expect(validateBotRule('ab')).toMatch(/Не короче 3/)
    expect(validateBotRule('x'.repeat(129))).toMatch(/Не длиннее 128/)
    expect(validateBotRule('a\u0007bc')).toMatch(/Недопустимые/)
    expect(validateBotRule('  AhrefsBot ')).toBeNull()
    expect(validateBotRule('Screaming Frog')).toBeNull()
  })

  it('warns before blocking risky categories only', () => {
    expect(botCategoryWarning('libraries')).toMatch(/curl/)
    expect(botCategoryWarning('empty')).toMatch(/User-Agent/)
    expect(botCategoryWarning('search')).toMatch(/индексировать/)
    expect(botCategoryWarning('seo')).toBeNull()
    expect(isBotList('botBlocked')).toBe(true)
    expect(isBotList('blacklist')).toBe(false)
  })

  it('finds the explicit rule of a bot case-insensitively', () => {
    const rules = { botBlocked: [{ value: 'AhrefsBot' }], botAllowed: [{ value: 'uptimerobot' }] }
    expect(botRuleOf('ahrefsbot', rules)).toEqual({ list: 'botBlocked', entry: { value: 'AhrefsBot' } })
    expect(botRuleOf('UptimeRobot', rules).list).toBe('botAllowed')
    expect(botRuleOf('GPTBot', rules)).toBeNull()
    expect(botRuleOf('GPTBot', {})).toBeNull()
  })
})
