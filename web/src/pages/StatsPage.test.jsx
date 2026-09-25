import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mockServer, renderApp } from '../test/render.jsx'

afterEach(() => vi.unstubAllGlobals())

const ROWS = [
  { trackerName: 'rutor', lastnewtor: '25.09.2026', newtor: 17, update: 983, check: 983, alltorrents: 983 },
  { trackerName: 'kinozal', lastnewtor: '01.08.2026', newtor: 0, update: 3, check: 3, alltorrents: 5000 },
]

describe('StatsPage', () => {
  it('renders totals and a sortable table', async () => {
    mockServer((url) => {
      if (url.pathname === '/stats/torrents') return [200, ROWS]
      if (url.pathname === '/stats/meta') return [200, { ok: true, updatedAt: '2026-09-25T08:39:44Z' }]
      if (url.pathname === '/lastupdatedb') return [200, { lastupdatedb: '25.09.2026 07:48' }]
      return [200, {}]
    })
    renderApp('/stats')
    const table = await screen.findByRole('table')
    expect(screen.getByText('25.09.2026 07:48')).toBeInTheDocument()
    expect(screen.getByText('Всего раздач').nextSibling.textContent.replace(/\s/g, '')).toBe('5983')

    let rows = within(table).getAllByRole('row').slice(1)
    expect(within(rows[0]).getByText('Kinozal')).toBeInTheDocument()

    await userEvent.setup().click(within(table).getByRole('button', { name: /Новых сегодня/ }))
    rows = within(table).getAllByRole('row').slice(1)
    expect(within(rows[0]).getByText('Rutor')).toBeInTheDocument()
    expect(within(rows[0]).getByText('+17')).toBeInTheDocument()
  })

  it('explains when statistics are disabled (openstats: false)', async () => {
    mockServer((url) => {
      if (url.pathname === '/stats/torrents') return [200, []]
      if (url.pathname === '/stats/meta') return [200, { ok: false }]
      return [200, {}]
    })
    renderApp('/stats')
    expect(await screen.findByText('Статистика скрыта')).toBeInTheDocument()
  })

  it('asks for the API key on 401', async () => {
    mockServer((url) => (url.pathname.startsWith('/stats') ? [401, {}] : [200, { configured: true, apikey: false }]))
    renderApp('/stats')
    expect(await screen.findByRole('heading', { name: 'Для поиска нужен API-ключ.' })).toBeInTheDocument()
  })
})
