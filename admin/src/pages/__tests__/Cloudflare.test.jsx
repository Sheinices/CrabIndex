import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { ToastProvider } from '../../components/Toast.jsx'
import { ConfirmProvider } from '../../components/Confirm.jsx'
import { setBaseForTests } from '../../lib/base.js'
import { CloudflarePage } from '../Cloudflare.jsx'

const STATUS = {
  enabled: true,
  paused: false,
  settings: { url: 'http://127.0.0.1:8191/v1', crawlUrl: '', maxTimeoutMs: 300000, sessionIdleMinutes: 120, recycleAfterTimeouts: 3, guardedHours: 6 },
  solver: { reachable: true, version: '3.5.2', sessions: ['crabindex-open_selezen_org'] },
  cffetch: { enabled: true, url: 'http://127.0.0.1:8192/fetch', impersonate: 'chrome136', hosts: [] },
  sessions: [{ name: 'crabindex-open_selezen_org', host: 'open.selezen.org', alive: true, busy: false, lastUse: '2026-09-26T08:00:00Z', consecutiveTimeouts: 0 }],
  guarded: [{ host: 'open.selezen.org', since: '2026-09-26T07:00:00Z' }],
  stats: {
    since: '2026-09-26T00:00:00Z',
    hosts: [
      {
        host: 'open.selezen.org',
        browserRequests: 10,
        browserOk: 4,
        browserFailed: 6,
        tabCrashed: 6,
        browserTimeouts: 0,
        challengeFailed: 0,
        sessionErrors: 0,
        unreachable: 0,
        pageFailed: 0,
        otherErrors: 0,
        sessionsCreated: 7,
        sessionsRecycled: 6,
        sessionsClosedIdle: 0,
        fastOk: 20,
        fastFailed: 2,
        totalBrowserMs: 50_000,
        lastErrorAt: '2026-09-26T08:10:00Z',
        lastError: 'Message: tab crashed',
      },
    ],
    recentErrors: [{ at: '2026-09-26T08:10:00Z', host: 'open.selezen.org', kind: 'tabCrashed', message: 'Message: tab crashed' }],
  },
}

function stubFetch() {
  const fn = vi.fn(async (url) => {
    const u = String(url)
    const body = u.includes('/cron/cloudflare/sessions/close') ? { ok: true, closed: 1, busy: 0 } : STATUS
    return new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } })
  })
  vi.stubGlobal('fetch', fn)
  return fn
}

function renderPage() {
  setBaseForTests('/admin')
  const router = createMemoryRouter([{ path: '/', element: <CloudflarePage /> }, { path: '/settings', element: <p>settings</p> }])
  return render(
    <ToastProvider>
      <ConfirmProvider>
        <RouterProvider router={router} />
      </ConfirmProvider>
    </ToastProvider>,
  )
}

describe('CloudflarePage', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('shows per-host stats and the memory advice for tab crashes', async () => {
    stubFetch()
    renderPage()
    expect(await screen.findByText('v3.5.2')).toBeInTheDocument()
    expect(screen.getByText('Вкладка упала: 6')).toBeInTheDocument()
    expect(screen.getByText(/Вкладки браузера падают по памяти: 6/)).toBeInTheDocument()
    expect(screen.getAllByText(/docker update --memory 4g/).length).toBeGreaterThan(0)
  })

  it('closes one host session through the admin API with the CSRF header', async () => {
    const fetch = stubFetch()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: 'Закрыть сессию open.selezen.org' }))
    await waitFor(() => expect(fetch.mock.calls.some(([u]) => String(u).includes('/api/cron/cloudflare/sessions/close?host=open.selezen.org'))).toBe(true))
    const call = fetch.mock.calls.find(([u]) => String(u).includes('/sessions/close'))
    expect(call[1].method).toBe('POST')
    expect(call[1].headers['X-Crab-Admin']).toBe('1')
  })
})
