import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { TimelineChart, chartPoints, niceMax } from './TimelineChart.jsx'

const SERIES = [
  { key: 'requests', label: 'Запросы', color: 'text-chart-1', area: true },
  { key: 'blocked', label: 'Заблокировано', color: 'text-chart-2' },
]
const DATA = [
  { t: '2026-09-25T10:00:00Z', requests: 10, blocked: 1 },
  { t: '2026-09-25T10:01:00Z', requests: 40, blocked: 4 },
  { t: '2026-09-25T10:02:00Z', requests: 20, blocked: 0 },
  { t: '2026-09-25T10:03:00Z', requests: 0, blocked: 0 },
]

describe('TimelineChart', () => {
  it('computes a readable axis ceiling', () => {
    expect(niceMax(0)).toBe(1)
    expect(niceMax(40)).toBe(50)
    expect(niceMax(180)).toBe(200)
    expect(niceMax(7)).toBe(10)
  })

  it('maps values to plot coordinates', () => {
    const pad = { top: 0, right: 0, bottom: 0, left: 0 }
    expect(chartPoints([0, 50, 100], { width: 200, height: 100, max: 100, pad })).toEqual([
      [0, 100],
      [100, 50],
      [200, 0],
    ])
  })

  it('renders one polyline point per data row for each series', () => {
    render(<TimelineChart data={DATA} series={SERIES} />)
    for (const key of ['requests', 'blocked']) {
      const pts = screen.getByTestId(`series-${key}`).getAttribute('points').trim().split(/\s+/)
      expect(pts).toHaveLength(DATA.length)
      pts.forEach((p) => expect(p).toMatch(/^[\d.]+,[\d.]+$/))
    }
    // The peak (40 of max 50) sits higher (smaller y) than the zero point.
    const req = screen.getByTestId('series-requests').getAttribute('points').split(' ').map((p) => Number(p.split(',')[1]))
    expect(req[1]).toBeLessThan(req[3])
    expect(screen.getByRole('img')).toHaveAccessibleName(/Запросы 70.*Заблокировано 5/)
  })

  it('shows an empty state and a table view', async () => {
    const { rerender } = render(<TimelineChart data={[]} series={SERIES} />)
    expect(screen.getByText('Нет данных')).toBeInTheDocument()
    rerender(<TimelineChart data={DATA} series={SERIES} />)
    await userEvent.click(screen.getByRole('button', { name: 'Таблица' }))
    expect(screen.getAllByRole('row')).toHaveLength(DATA.length + 1)
  })
})
