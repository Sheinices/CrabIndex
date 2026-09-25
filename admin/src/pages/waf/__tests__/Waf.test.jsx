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

const RULES = {
  blacklist: [{ value: '203.0.113.7', comment: 'сканер', created: '2026-09-20T10:00:00Z', expires: null }],
  whitelist: [{ value: '198.51.100.0/24', comment: 'офис', created: '2026-09-20T10:00:00Z', expires: null }],
  bans: [{ ip: '192.0.2.10', reason: 'rate', created: '2026-09-25T10:00:00Z', expires: '2099-01-01T00:00:00Z' }],
  config: { enable: true, logRequests: true, historySize: 5000, rateLimit: { enable: true, perMinute: 300, banMinutes: 15 }, trapPaths: ['/.env'], trapBanMinutes: 1440, blockUserAgents: [], whitelistLan: true },
  you: '192.168.1.10',
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

    const section = screen.getByRole('region', { name: /Чёрный список/ })
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
})
