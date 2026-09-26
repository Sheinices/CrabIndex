import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { ToastProvider } from '../../../components/Toast.jsx'
import { ConfirmProvider } from '../../../components/Confirm.jsx'
import { setBaseForTests } from '../../../lib/base.js'
import { WafPage } from '../Waf.jsx'
import { WafRules } from '../WafRules.jsx'
import { WafLog } from '../WafLog.jsx'
import { WafBots } from '../WafBots.jsx'

const RULES = {
  blacklist: [{ value: '203.0.113.7', comment: 'сканер', created: '2026-09-20T10:00:00Z', expires: null }],
  whitelist: [{ value: '198.51.100.0/24', comment: 'офис', created: '2026-09-20T10:00:00Z', expires: null }],
  bans: [{ ip: '192.0.2.10', reason: 'rate', created: '2026-09-25T10:00:00Z', expires: '2099-01-01T00:00:00Z' }],
  domainBlacklist: [{ value: 'spam.example', comment: 'парсер', created: '2026-09-20T10:00:00Z', expires: null }],
  domainWhitelist: [{ value: 'friend.example', comment: '', created: '2026-09-20T10:00:00Z', expires: null }],
  builtinDomains: ['ndst.pw', 'myds.me', 'lampa.stream'],
  config: {
    enable: true,
    logRequests: true,
    historySize: 5000,
    rateLimit: { enable: true, perMinute: 300, banMinutes: 15 },
    trapPaths: ['/.env'],
    trapBanMinutes: 1440,
    blockUserAgents: [],
    whitelistLan: true,
    domainAllowlistOnly: true,
  },
  you: '192.168.1.10',
}

const BOTS = {
  categories: [
    { id: 'search', label: 'Поисковые роботы', description: 'Индексируют сайты', requests: 12, blocked: 0, blockedCategory: false, botCount: 1 },
    { id: 'seo', label: 'SEO-сервисы', description: 'Анализ ссылок', requests: 40, blocked: 5, blockedCategory: false, botCount: 1 },
    { id: 'libraries', label: 'HTTP-библиотеки и утилиты', description: 'Скрипты', requests: 7, blocked: 0, blockedCategory: false, botCount: 1 },
    { id: 'ai', label: 'AI-краулеры', description: 'Нейросети', requests: 3, blocked: 3, blockedCategory: true, botCount: 1 },
  ],
  bots: [
    {
      name: 'AhrefsBot',
      category: 'seo',
      requests: 40,
      blocked: 5,
      lastSeen: '2026-09-25T10:00:00Z',
      ips: 2,
      topPaths: [{ path: '/api/v1.0/torrents', requests: 30 }],
      samples: ['Mozilla/5.0 (compatible; AhrefsBot/7.0)'],
      status: 'seen',
    },
    { name: 'GPTBot', category: 'ai', requests: 3, blocked: 3, lastSeen: '2026-09-25T10:00:00Z', ips: 1, topPaths: [], samples: [], status: 'blocked' },
    { name: 'curl', category: 'libraries', requests: 7, blocked: 0, lastSeen: '2026-09-25T10:00:00Z', ips: 1, topPaths: [], samples: ['curl/8.9.1'], status: 'allowed' },
  ],
  rules: {
    botBlockCategories: ['ai'],
    botBlocked: [],
    botAllowed: [{ value: 'curl', comment: 'свой cron', created: '2026-09-20T10:00:00Z', expires: null }],
    robotsDisallow: false,
  },
  catalog: { search: ['Googlebot', 'YandexBot'], seo: ['AhrefsBot'], libraries: ['curl'], ai: ['GPTBot'] },
}

const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

/** Fetch stub: `overrides[METHOD path]` returns a Response, otherwise sensible defaults. */
function stubFetch(overrides = {}) {
  const fn = vi.fn(async (url, init = {}) => {
    const u = new URL(String(url), 'http://x')
    const key = `${init.method || 'GET'} ${u.pathname.replace('/admin/api/', '')}`
    if (overrides[key]) return overrides[key](u, init)
    if (key === 'GET waf/rules') return json(RULES)
    if (key === 'GET waf/requests') return json([])
    if (key === 'GET waf/bots') return json(BOTS)
    return json({ ok: true })
  })
  vi.stubGlobal('fetch', fn)
  return fn
}

function renderAt(path) {
  const router = createMemoryRouter(
    [
      {
        path: '/waf',
        element: <WafPage />,
        children: [
          { path: 'log', element: <WafLog /> },
          { path: 'rules', element: <WafRules /> },
          { path: 'bots', element: <WafBots /> },
        ],
      },
    ],
    { initialEntries: [path] },
  )
  render(
    <ToastProvider>
      <ConfirmProvider>
        <RouterProvider router={router} />
      </ConfirmProvider>
    </ToastProvider>,
  )
  return router
}

const calls = (fetch, method, path) =>
  fetch.mock.calls.filter(([u, init = {}]) => (init.method || 'GET') === method && new URL(String(u), 'http://x').pathname === `/admin/api/${path}`)

describe('WAF rules tab', () => {
  beforeEach(() => setBaseForTests('/admin'))
  afterEach(() => vi.unstubAllGlobals())

  it('validates client-side, then POSTs a blacklist entry', async () => {
    const fetch = stubFetch()
    renderAt('/waf/rules')
    expect(await screen.findByText('203.0.113.7')).toBeInTheDocument()
    expect(screen.getByRole('note')).toHaveTextContent('192.168.1.10')

    const section = screen.getByRole('region', { name: /^Чёрный список ·/ })
    await userEvent.click(within(section).getByRole('button', { name: /Заблокировать/ }))
    const dialog = await screen.findByRole('dialog')
    const input = within(dialog).getByLabelText('IP-адрес или CIDR')

    await userEvent.type(input, '300.1.1.1')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent(/Некорректный/)

    await userEvent.clear(input)
    await userEvent.type(input, '192.168.1.0/24')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent(/собственный IP/)
    expect(calls(fetch, 'POST', 'waf/rules')).toHaveLength(0)

    await userEvent.clear(input)
    await userEvent.type(input, '45.155.205.0/24')
    await userEvent.type(within(dialog).getByLabelText(/Комментарий/), 'ботнет')
    await userEvent.selectOptions(within(dialog).getByLabelText('Срок действия'), '1440')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))

    await waitFor(() => expect(calls(fetch, 'POST', 'waf/rules')).toHaveLength(1))
    const [, init] = calls(fetch, 'POST', 'waf/rules')[0]
    expect(init.headers['X-Crab-Admin']).toBe('1')
    expect(JSON.parse(init.body)).toEqual({ list: 'blacklist', value: '45.155.205.0/24', comment: 'ботнет', expiresMinutes: 1440 })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(await screen.findByText('Добавлено: 45.155.205.0/24')).toBeInTheDocument()
  })

  it('deletes a whitelist entry only after confirmation', async () => {
    const fetch = stubFetch()
    renderAt('/waf/rules')
    await userEvent.click(await screen.findByRole('button', { name: 'Удалить 198.51.100.0/24 из списка «Белый список»' }))
    const dialog = await screen.findByRole('dialog', { name: 'Удалить правило?' })
    expect(calls(fetch, 'DELETE', 'waf/rules')).toHaveLength(0)
    await userEvent.click(within(dialog).getByRole('button', { name: 'Удалить' }))
    await waitFor(() => expect(calls(fetch, 'DELETE', 'waf/rules')).toHaveLength(1))
    const url = new URL(String(calls(fetch, 'DELETE', 'waf/rules')[0][0]), 'http://x')
    expect(url.searchParams.get('list')).toBe('whitelist')
    expect(url.searchParams.get('value')).toBe('198.51.100.0/24')
  })

  it('lifts a ban and shows server errors in a toast', async () => {
    const fetch = stubFetch({ 'DELETE waf/ban': () => json({ ok: false, error: 'no active ban for 192.0.2.10' }, 400) })
    renderAt('/waf/rules')
    await userEvent.click(await screen.findByRole('button', { name: 'Снять бан' }))
    await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Снять бан' }))
    await waitFor(() => expect(calls(fetch, 'DELETE', 'waf/ban')).toHaveLength(1))
    expect(new URL(String(calls(fetch, 'DELETE', 'waf/ban')[0][0]), 'http://x').searchParams.get('ip')).toBe('192.0.2.10')
    const toast = await screen.findByText('no active ban for 192.0.2.10')
    expect(toast.closest('[role="alert"]')).toHaveTextContent('Ошибка WAF')
  })

  it('renders the builtin domain list read-only with the allowlist-only state', async () => {
    stubFetch()
    renderAt('/waf/rules')
    const builtin = await screen.findByRole('region', { name: /Встроенные заблокированные домены/ })
    expect(builtin).toHaveTextContent('Встроенный список, изменить нельзя')
    const items = within(within(builtin).getByRole('list', { name: 'Встроенный список доменов' })).getAllByRole('listitem')
    expect(items.map((li) => li.textContent)).toEqual(['ndst.pw', 'myds.me', 'lampa.stream'])
    expect(within(builtin).queryAllByRole('button')).toHaveLength(0)
    expect(screen.queryByRole('button', { name: /Удалить ndst\.pw/ })).not.toBeInTheDocument()
    expect(screen.getByText('включено')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /Изменить в настройках/ })).toHaveAttribute('href', '/settings?group=waf')
  })

  it('validates and adds a domain, refusing builtin conflicts client-side', async () => {
    const fetch = stubFetch()
    renderAt('/waf/rules')
    const section = await screen.findByRole('region', { name: /^Белый список доменов/ })
    expect(within(section).getByText('friend.example')).toBeInTheDocument()
    await userEvent.click(within(section).getByRole('button', { name: /Разрешить/ }))
    const dialog = await screen.findByRole('dialog')
    const input = within(dialog).getByLabelText('Домен')

    await userEvent.type(input, 'no_dot')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Добавить' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent(/Некорректный домен/)

    await userEvent.clear(input)
    await userEvent.type(input, 'https://app.ndst.pw/')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Добавить' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent('app.ndst.pw заблокирован встроенным списком и не может быть разрешён')
    expect(calls(fetch, 'POST', 'waf/rules')).toHaveLength(0)

    await userEvent.clear(input)
    await userEvent.type(input, '*.My-Lampa.example')
    await userEvent.type(within(dialog).getByLabelText(/Комментарий/), 'своя')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Добавить' }))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/rules')).toHaveLength(1))
    expect(JSON.parse(calls(fetch, 'POST', 'waf/rules')[0][1].body)).toEqual({ list: 'domainWhitelist', value: 'my-lampa.example', comment: 'своя' })
    expect(await screen.findByText('Добавлено: my-lampa.example')).toBeInTheDocument()
  })

  it('shows a server-side builtin conflict as an error', async () => {
    stubFetch({ 'POST waf/rules': () => json({ ok: false, error: 'x.lampa.stream заблокирован встроенным списком и не может быть разрешён' }, 400) })
    renderAt('/waf/rules')
    const section = await screen.findByRole('region', { name: /^Чёрный список доменов/ })
    await userEvent.click(within(section).getByRole('button', { name: /Заблокировать/ }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.type(within(dialog).getByLabelText('Домен'), 'other.example')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))
    expect(await screen.findByText('x.lampa.stream заблокирован встроенным списком и не может быть разрешён')).toBeInTheDocument()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('deletes a domain blacklist entry after confirmation', async () => {
    const fetch = stubFetch()
    renderAt('/waf/rules')
    await userEvent.click(await screen.findByRole('button', { name: 'Удалить spam.example из списка «Чёрный список доменов»' }))
    const dialog = await screen.findByRole('dialog', { name: 'Удалить правило?' })
    expect(calls(fetch, 'DELETE', 'waf/rules')).toHaveLength(0)
    await userEvent.click(within(dialog).getByRole('button', { name: 'Удалить' }))
    await waitFor(() => expect(calls(fetch, 'DELETE', 'waf/rules')).toHaveLength(1))
    const url = new URL(String(calls(fetch, 'DELETE', 'waf/rules')[0][0]), 'http://x')
    expect(url.searchParams.get('list')).toBe('domainBlacklist')
    expect(url.searchParams.get('value')).toBe('spam.example')
  })

  it('resets statistics after confirmation', async () => {
    const fetch = stubFetch()
    renderAt('/waf/rules')
    await userEvent.click(await screen.findByRole('button', { name: /Сбросить статистику/ }))
    await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Сбросить' }))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/reset')).toHaveLength(1))
  })

  it('shows the disabled notice with a settings link', async () => {
    stubFetch({ 'GET waf/rules': () => json({ ...RULES, config: { ...RULES.config, enable: false } }) })
    renderAt('/waf/rules')
    expect(await screen.findByText('WAF выключен')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Открыть настройки WAF' })).toHaveAttribute('href', '/settings?group=waf')
  })
})

describe('WAF log filters', () => {
  beforeEach(() => setBaseForTests('/admin'))
  afterEach(() => vi.unstubAllGlobals())

  const lastQuery = (fetch) => {
    const list = calls(fetch, 'GET', 'waf/requests')
    return Object.fromEntries(new URL(String(list[list.length - 1][0]), 'http://x').searchParams)
  }

  it('builds the query from URL filters and controls', async () => {
    const fetch = stubFetch({
      'GET waf/requests': () =>
        json([{ time: '2026-09-25T10:00:00Z', ip: '5.188.62.140', method: 'GET', path: '/api/v1.0/torrents', status: 429, ms: 0, ua: 'curl/8', blocked: 'rate' }]),
    })
    renderAt('/waf/log?ip=203.0.113.7')
    await waitFor(() => expect(lastQuery(fetch)).toEqual({ ip: '203.0.113.7', limit: '200' }))
    expect(await screen.findByText('Лимит запросов')).toBeInTheDocument()
    expect(within(screen.getByRole('table')).getByText('429')).toHaveClass('text-danger')

    await userEvent.selectOptions(screen.getByLabelText('Статус'), '4xx')
    await waitFor(() => expect(lastQuery(fetch)).toMatchObject({ status: '4xx' }))

    await userEvent.click(screen.getByLabelText('Только заблокированные'))
    await waitFor(() => expect(lastQuery(fetch)).toEqual({ ip: '203.0.113.7', status: '4xx', blocked: 'true', limit: '200' }))

    const path = screen.getByLabelText('Путь (содержит)')
    await userEvent.type(path, '/api{Enter}')
    await waitFor(() => expect(lastQuery(fetch)).toMatchObject({ path: '/api' }))

    // Clicking an IP in the log switches the IP filter.
    await userEvent.click(screen.getByRole('button', { name: '5.188.62.140' }))
    await waitFor(() => expect(lastQuery(fetch)).toMatchObject({ ip: '5.188.62.140' }))
    expect(screen.getByLabelText('IP')).toHaveValue('5.188.62.140')
  })

  it('shows the origin column and filters by origin', async () => {
    const fetch = stubFetch({
      'GET waf/requests': () =>
        json([{ time: '2026-09-25T10:00:00Z', ip: '176.59.40.12', method: 'GET', path: '/api/v1.0/torrents', status: 403, ms: 0, ua: 'Lampa', blocked: 'domain', origin: 'app.ndst.pw' }]),
    })
    renderAt('/waf/log')
    expect(await screen.findByText('Домен', { selector: '.badge' })).toBeInTheDocument()
    expect(within(screen.getByRole('table')).getByRole('columnheader', { name: 'Origin' })).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'app.ndst.pw' }))
    await waitFor(() => expect(lastQuery(fetch)).toMatchObject({ origin: 'app.ndst.pw' }))
    expect(screen.getByLabelText('Домен (Origin)')).toHaveValue('app.ndst.pw')
    await userEvent.clear(screen.getByLabelText('Домен (Origin)'))
    await userEvent.type(screen.getByLabelText('Домен (Origin)'), 'LAMPA{Enter}')
    await waitFor(() => expect(lastQuery(fetch)).toEqual({ origin: 'lampa', limit: '200' }))
  })
})

describe('WAF log hosts', () => {
  beforeEach(() => setBaseForTests('/admin'))
  afterEach(() => vi.unstubAllGlobals())

  it('shows the host column and filters by host', async () => {
    const fetch = stubFetch({
      'GET waf/requests': () =>
        json([{ time: '2026-09-25T10:00:00Z', ip: '203.0.113.9', method: 'GET', path: '/', status: 200, ms: 1, ua: 'x', blocked: null, origin: null, host: '198.51.100.1' }]),
    })
    renderAt('/waf/log')
    expect(await within(await screen.findByRole('table')).findByRole('columnheader', { name: 'Хост' })).toBeInTheDocument()
    await userEvent.click(await screen.findByRole('button', { name: '198.51.100.1' }))
    const last = () => {
      const list = calls(fetch, 'GET', 'waf/requests')
      return Object.fromEntries(new URL(String(list[list.length - 1][0]), 'http://x').searchParams)
    }
    await waitFor(() => expect(last()).toMatchObject({ host: '198.51.100.1' }))
    expect(screen.getByLabelText('Хост (Host)')).toHaveValue('198.51.100.1')
  })
})

describe('WAF bots tab', () => {
  beforeEach(() => setBaseForTests('/admin'))
  afterEach(() => vi.unstubAllGlobals())

  it('renders categories, seen bots and the explanation', async () => {
    stubFetch()
    renderAt('/waf/bots')
    expect(await screen.findByRole('link', { name: 'Боты' })).toHaveAttribute('aria-current', 'page')
    const seo = await screen.findByRole('region', { name: 'SEO-сервисы' })
    expect(seo).toHaveTextContent('40')
    expect(within(screen.getByRole('region', { name: 'AI-краулеры' })).getByLabelText('Блокировать всю категорию')).toBeChecked()
    expect(within(screen.getByRole('region', { name: 'HTTP-библиотеки и утилиты' })).getByText(/curl, python-requests/)).toBeInTheDocument()
    expect(screen.getByRole('note')).toHaveTextContent('Jackett')
    const table = screen.getByRole('table')
    expect(within(table).getByText('AhrefsBot')).toBeInTheDocument()
    expect(within(table).getByText('блокируется')).toBeInTheDocument()
    expect(within(table).getByRole('button', { name: 'Снять правило для curl' })).toBeInTheDocument()
    expect(within(table).queryByRole('button', { name: 'Блокировать GPTBot' })).not.toBeInTheDocument()
    expect(within(table).getByRole('button', { name: 'Разрешить GPTBot' })).toBeInTheDocument()
  })

  it('blocks a category after confirmation with the admin header', async () => {
    const fetch = stubFetch()
    renderAt('/waf/bots')
    const seo = await screen.findByRole('region', { name: 'SEO-сервисы' })
    await userEvent.click(within(seo).getByLabelText('Блокировать всю категорию'))
    const dialog = await screen.findByRole('dialog', { name: 'Блокировать категорию «SEO-сервисы»?' })
    expect(calls(fetch, 'POST', 'waf/bots/category')).toHaveLength(0)
    await userEvent.click(within(dialog).getByRole('button', { name: 'Блокировать' }))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/bots/category')).toHaveLength(1))
    const [, init] = calls(fetch, 'POST', 'waf/bots/category')[0]
    expect(init.headers['X-Crab-Admin']).toBe('1')
    expect(JSON.parse(init.body)).toEqual({ id: 'seo', block: true })
    expect(await screen.findByText('Категория заблокирована: SEO-сервисы')).toBeInTheDocument()
  })

  it('warns before blocking libraries and does nothing on cancel', async () => {
    const fetch = stubFetch()
    renderAt('/waf/bots')
    const libs = await screen.findByRole('region', { name: 'HTTP-библиотеки и утилиты' })
    await userEvent.click(within(libs).getByLabelText('Блокировать всю категорию'))
    const dialog = await screen.findByRole('dialog')
    expect(dialog).toHaveTextContent(/легитимные скрипты/)
    await userEvent.click(within(dialog).getByRole('button', { name: 'Отмена' }))
    expect(calls(fetch, 'POST', 'waf/bots/category')).toHaveLength(0)
  })

  it('blocks a seen bot, adds a custom rule and toggles robots.txt', async () => {
    const fetch = stubFetch()
    renderAt('/waf/bots')
    await userEvent.click(await screen.findByRole('button', { name: 'Блокировать AhrefsBot' }))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/bots/rules')).toHaveLength(1))
    expect(JSON.parse(calls(fetch, 'POST', 'waf/bots/rules')[0][1].body)).toEqual({ list: 'botBlocked', value: 'AhrefsBot' })

    const section = screen.getByRole('region', { name: /^Заблокированные боты/ })
    await userEvent.click(within(section).getByRole('button', { name: /Заблокировать/ }))
    const dialog = await screen.findByRole('dialog')
    const input = within(dialog).getByLabelText('Имя бота или часть User-Agent')
    await userEvent.type(input, 'ab')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))
    expect(within(dialog).getByRole('alert')).toHaveTextContent(/Не короче 3/)
    await userEvent.clear(input)
    await userEvent.type(input, 'MyScraper/')
    await userEvent.type(within(dialog).getByLabelText(/Комментарий/), 'парсер')
    await userEvent.selectOptions(within(dialog).getByLabelText('Срок действия'), '60')
    await userEvent.click(within(dialog).getByRole('button', { name: 'Заблокировать' }))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/bots/rules')).toHaveLength(2))
    expect(JSON.parse(calls(fetch, 'POST', 'waf/bots/rules')[1][1].body)).toEqual({ list: 'botBlocked', value: 'MyScraper/', comment: 'парсер', expiresMinutes: 60 })

    await userEvent.click(screen.getByLabelText('robots.txt: запретить индексацию'))
    await waitFor(() => expect(calls(fetch, 'POST', 'waf/bots/robots')).toHaveLength(1))
    expect(JSON.parse(calls(fetch, 'POST', 'waf/bots/robots')[0][1].body)).toEqual({ disallow: true })
  })

  it('removes a bot rule after confirmation', async () => {
    const fetch = stubFetch()
    renderAt('/waf/bots')
    await userEvent.click(await screen.findByRole('button', { name: 'Снять правило для curl' }))
    const dialog = await screen.findByRole('dialog', { name: 'Снять правило?' })
    await userEvent.click(within(dialog).getByRole('button', { name: 'Снять правило' }))
    await waitFor(() => expect(calls(fetch, 'DELETE', 'waf/bots/rules')).toHaveLength(1))
    const url = new URL(String(calls(fetch, 'DELETE', 'waf/bots/rules')[0][0]), 'http://x')
    expect(url.searchParams.get('list')).toBe('botAllowed')
    expect(url.searchParams.get('value')).toBe('curl')
  })
})
