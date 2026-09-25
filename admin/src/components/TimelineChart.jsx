import { useEffect, useMemo, useRef, useState } from 'react'
import { formatNumber } from '../lib/format.js'

const PAD = { top: 12, right: 12, bottom: 26, left: 44 }

/** Round `max` up to a readable axis ceiling (1, 2, 2.5, 5 × 10^n). */
export function niceMax(max) {
  const v = Number(max)
  if (!(v > 0)) return 1
  const pow = 10 ** Math.floor(Math.log10(v))
  for (const m of [1, 2, 2.5, 5, 10]) if (v <= m * pow) return m * pow
  return 10 * pow
}

/** Plot coordinates `[[x, y], …]` for `values` inside a `width`×`height` box. */
export function chartPoints(values, { width, height, max, pad = PAD }) {
  const n = values.length
  const innerW = Math.max(1, width - pad.left - pad.right)
  const innerH = Math.max(1, height - pad.top - pad.bottom)
  const top = max > 0 ? max : 1
  return values.map((v, i) => {
    const x = pad.left + (n > 1 ? (i * innerW) / (n - 1) : innerW / 2)
    const y = pad.top + innerH - (Math.max(0, Number(v) || 0) / top) * innerH
    return [Math.round(x * 10) / 10, Math.round(y * 10) / 10]
  })
}

const toPoints = (pts) => pts.map(([x, y]) => `${x},${y}`).join(' ')

function defaultTime(t) {
  const d = new Date(t)
  return Number.isNaN(d.getTime()) ? String(t ?? '') : d.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })
}

/**
 * Dependency-free SVG line/area chart for a time series.
 * `series`: `[{ key, label, color: 'text-chart-1', area?: boolean }]`.
 */
export function TimelineChart({ data, series, height = 220, formatX = defaultTime, label = 'График' }) {
  const wrapRef = useRef(null)
  const [width, setWidth] = useState(720)
  const [hover, setHover] = useState(null)
  const [showTable, setShowTable] = useState(false)
  const rows = useMemo(() => (Array.isArray(data) ? data : []), [data])

  useEffect(() => {
    const el = wrapRef.current
    if (!el || typeof ResizeObserver === 'undefined') return undefined
    const ro = new ResizeObserver(([entry]) => {
      const w = Math.round(entry.contentRect.width)
      if (w > 0) setWidth(w)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const max = useMemo(() => niceMax(Math.max(0, ...rows.flatMap((r) => series.map((s) => Number(r[s.key]) || 0)))), [rows, series])
  const lines = useMemo(
    () => series.map((s) => ({ ...s, pts: chartPoints(rows.map((r) => r[s.key]), { width, height, max }) })),
    [rows, series, width, height, max],
  )
  const totals = useMemo(() => Object.fromEntries(series.map((s) => [s.key, rows.reduce((a, r) => a + (Number(r[s.key]) || 0), 0)])), [rows, series])

  const baseY = height - PAD.bottom
  const innerW = width - PAD.left - PAD.right
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((f) => ({ v: max * f, y: PAD.top + (baseY - PAD.top) * (1 - f) }))
  const xLabelCount = Math.min(rows.length, Math.max(2, Math.floor(innerW / 90)))
  const xLabels =
    rows.length > 1
      ? Array.from({ length: xLabelCount }, (_, k) => Math.round((k * (rows.length - 1)) / (xLabelCount - 1)))
      : rows.length
        ? [0]
        : []

  const onMove = (e) => {
    if (!rows.length) return
    const rect = e.currentTarget.getBoundingClientRect()
    const scale = rect.width ? width / rect.width : 1
    const x = (e.clientX - rect.left) * scale
    const i = rows.length > 1 ? Math.round(((x - PAD.left) / innerW) * (rows.length - 1)) : 0
    setHover(Math.max(0, Math.min(rows.length - 1, i)))
  }

  const hx = hover != null ? lines[0]?.pts[hover]?.[0] : null

  return (
    <figure className="m-0">
      <div className="mb-2 flex flex-wrap items-center gap-4 text-xs text-muted">
        {series.map((s) => (
          <span key={s.key} className="inline-flex items-center gap-1.5" aria-hidden="true">
            <span className={`inline-block h-0.5 w-4 rounded-full bg-current ${s.color}`} />
            {s.label}
            <span className="text-fg tabular-nums">{formatNumber(totals[s.key])}</span>
          </span>
        ))}
        <button type="button" className="ml-auto text-accent hover:underline" onClick={() => setShowTable((v) => !v)}>
          {showTable ? 'График' : 'Таблица'}
        </button>
      </div>
      {showTable ? (
        <div className="max-h-64 overflow-auto rounded-lg border border-border">
          <table className="table">
            <thead>
              <tr>
                <th>Время</th>
                {series.map((s) => (
                  <th key={s.key} className="text-right">
                    {s.label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={`${r.t}-${i}`}>
                  <td className="tabular-nums">{formatX(r.t)}</td>
                  {series.map((s) => (
                    <td key={s.key} className="text-right tabular-nums">
                      {formatNumber(r[s.key])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div ref={wrapRef} className="relative w-full">
          <svg
            role="img"
            aria-label={`${label}: ${series.map((s) => `${s.label} ${formatNumber(totals[s.key])}`).join(', ')}`}
            width="100%"
            height={height}
            viewBox={`0 0 ${width} ${height}`}
            className="block overflow-visible"
            onMouseMove={onMove}
            onMouseLeave={() => setHover(null)}
          >
            {ticks.map((t) => (
              <g key={t.y}>
                <line x1={PAD.left} x2={width - PAD.right} y1={t.y} y2={t.y} className="stroke-border" strokeWidth="1" />
                <text x={PAD.left - 6} y={t.y} dy="0.32em" textAnchor="end" className="fill-muted text-[10px] tabular-nums">
                  {formatNumber(Math.round(t.v * 10) / 10)}
                </text>
              </g>
            ))}
            {xLabels.map((i) => (
              <text
                key={i}
                x={lines[0]?.pts[i]?.[0] ?? PAD.left}
                y={height - 6}
                textAnchor={i === 0 ? 'start' : i === rows.length - 1 ? 'end' : 'middle'}
                className="fill-muted text-[10px] tabular-nums"
              >
                {formatX(rows[i]?.t)}
              </text>
            ))}
            {lines.map((s) =>
              s.pts.length ? (
                <g key={s.key} className={s.color}>
                  {s.area ? (
                    <polygon
                      points={`${s.pts[0][0]},${baseY} ${toPoints(s.pts)} ${s.pts[s.pts.length - 1][0]},${baseY}`}
                      fill="currentColor"
                      fillOpacity="0.12"
                    />
                  ) : null}
                  <polyline
                    data-testid={`series-${s.key}`}
                    points={toPoints(s.pts)}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinejoin="round"
                    strokeLinecap="round"
                    vectorEffect="non-scaling-stroke"
                  />
                </g>
              ) : null,
            )}
            {hx != null ? (
              <g pointerEvents="none">
                <line x1={hx} x2={hx} y1={PAD.top} y2={baseY} className="stroke-muted" strokeWidth="1" strokeDasharray="3 3" />
                {lines.map((s) => (
                  <circle key={s.key} cx={s.pts[hover][0]} cy={s.pts[hover][1]} r="4" className={`${s.color} stroke-surface`} fill="currentColor" strokeWidth="2" />
                ))}
              </g>
            ) : null}
            {!rows.length ? (
              <text x={width / 2} y={height / 2} textAnchor="middle" className="fill-muted text-xs">
                Нет данных
              </text>
            ) : null}
          </svg>
          {hover != null && rows[hover] ? (
            <div
              className="pointer-events-none absolute top-2 z-10 rounded-lg border border-border bg-surface px-3 py-2 text-xs shadow-lg"
              style={hx > width / 2 ? { right: `${((width - hx) / width) * 100 + 2}%` } : { left: `${(hx / width) * 100 + 2}%` }}
            >
              <p className="mb-1 font-medium tabular-nums">{formatX(rows[hover].t)}</p>
              {series.map((s) => (
                <p key={s.key} className="flex items-center gap-2">
                  <span className={`inline-block size-2 rounded-full bg-current ${s.color}`} aria-hidden="true" />
                  <span className="text-muted">{s.label}</span>
                  <span className="ml-auto pl-3 tabular-nums">{formatNumber(rows[hover][s.key])}</span>
                </p>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </figure>
  )
}
