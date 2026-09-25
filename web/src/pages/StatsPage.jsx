import { ArrowDown, ArrowUp, ChartColumn, CircleAlert, KeyRound, Lock, RefreshCw, Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { ApiKeyForm } from '../components/ApiKeyForm.jsx'
import { useApp } from '../context.js'
import { getLastUpdateDb, getStatsMeta, getTrackerStats } from '../lib/api.js'
import { barPercent, formatNumber, sortTrackerStats, statsTotals } from '../lib/stats.js'
import { formatDateTime, trackerIcon, trackerLabel } from '../lib/torrents.js'

function todayRu() {
  const d = new Date()
  return `${String(d.getDate()).padStart(2, '0')}.${String(d.getMonth() + 1).padStart(2, '0')}.${d.getFullYear()}`
}

function Tile({ label, value, accent }) {
  return (
    <div className="card p-4 sm:p-5">
      <div className="text-xs font-medium text-muted">{label}</div>
      <div className={`mt-1.5 text-2xl font-semibold tracking-tight tabular-nums sm:text-3xl ${accent ? 'text-brand' : ''}`}>{value}</div>
    </div>
  )
}

function Message({ icon: Icon, title, children, tone }) {
  return (
    <div className="card flex flex-col items-center px-6 py-14 text-center">
      <div className={`mb-4 flex size-12 items-center justify-center rounded-full ${tone === 'error' ? 'bg-red-500/10 text-red-500' : 'bg-surface-2 text-muted'}`}>
        <Icon className="size-6" aria-hidden />
      </div>
      <h2 className="text-lg font-semibold">{title}</h2>
      <div className="mt-2 max-w-md text-sm text-muted">{children}</div>
    </div>
  )
}

function SortHeader({ id, label, sort, onSort, className = '', align = 'right' }) {
  const active = sort.key === id
  const Icon = sort.dir === 'asc' ? ArrowUp : ArrowDown
  return (
    <th scope="col" aria-sort={active ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'} className={`px-3 py-2.5 font-medium sm:px-4 ${className}`}>
      <button
        type="button"
        onClick={() => onSort(id)}
        className={`inline-flex items-center gap-1 rounded hover:text-fg ${align === 'right' ? 'flex-row-reverse' : ''} ${active ? 'text-fg' : ''}`}
      >
        {label}
        <Icon className={`size-3.5 ${active ? 'opacity-100' : 'opacity-0'}`} aria-hidden />
      </button>
    </th>
  )
}

export function StatsPage() {
  const { t, locale, apiKey } = useApp()
  const [attempt, setAttempt] = useState(0)
  const [data, setData] = useState({ status: 'loading', rows: [], meta: null, lastDb: '', error: null })
  const [sort, setSort] = useState({ key: 'alltorrents', dir: 'desc' })
  const [filter, setFilter] = useState('')

  useEffect(() => {
    document.title = `${t('stats.title')} - CrabIndex`
  }, [t])

  useEffect(() => {
    const ctrl = new AbortController()
    const opts = { signal: ctrl.signal }
    Promise.all([
      getTrackerStats(apiKey, opts),
      getStatsMeta(apiKey, opts).catch((e) => (e?.name === 'AbortError' ? Promise.reject(e) : null)),
      getLastUpdateDb(opts).catch((e) => (e?.name === 'AbortError' ? Promise.reject(e) : null)),
    ])
      .then(([rows, meta, lastDb]) => {
        const closed = meta?.ok === false && rows.length === 0
        setData({
          status: closed ? 'closed' : rows.length ? 'done' : 'empty',
          rows,
          meta,
          lastDb: lastDb?.lastupdatedb || '',
          error: null,
        })
      })
      .catch((error) => {
        if (error?.name === 'AbortError') return
        setData({ status: 'error', rows: [], meta: null, lastDb: '', error })
      })
    return () => ctrl.abort()
  }, [apiKey, attempt])

  const totals = useMemo(() => statsTotals(data.rows), [data.rows])
  const max = useMemo(() => Math.max(0, ...data.rows.map((r) => Number(r.alltorrents) || 0)), [data.rows])
  const rows = useMemo(() => {
    const q = filter.trim().toLowerCase()
    const list = q
      ? data.rows.filter((r) => String(r.trackerName).toLowerCase().includes(q) || trackerLabel(r.trackerName).toLowerCase().includes(q))
      : data.rows
    return sortTrackerStats(list, sort.key, sort.dir)
  }, [data.rows, filter, sort])

  const onSort = (key) =>
    setSort((s) => (s.key === key ? { key, dir: s.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'name' ? 'asc' : 'desc' }))

  const today = todayRu()
  const lastNewLabel = (v) => (!v ? '-' : v === today ? t('stats.today') : v)
  const n = (v) => formatNumber(v, locale)

  let content
  if (data.status === 'loading') {
    content = (
      <div aria-busy="true" className="space-y-4">
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="card p-5">
              <div className="skeleton h-3 w-20" />
              <div className="skeleton mt-3 h-7 w-24" />
            </div>
          ))}
        </div>
        <div className="card space-y-3 p-5">
          {[0, 1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton h-6 w-full" />
          ))}
        </div>
      </div>
    )
  } else if (data.status === 'error' && data.error?.unauthorized) {
    content = (
      <Message icon={KeyRound} title={t('apikey.required')}>
        <div className="mt-3 w-full max-w-sm text-left">
          <ApiKeyForm compact />
        </div>
      </Message>
    )
  } else if (data.status === 'error') {
    content = (
      <Message icon={CircleAlert} title={t('stats.errorTitle')} tone="error">
        {data.error?.status ? t('search.errorHttp', { status: data.error.status }) : t('search.errorNetwork')}
      </Message>
    )
  } else if (data.status === 'closed') {
    content = (
      <Message icon={Lock} title={t('stats.closedTitle')}>
        {t('stats.closedText')}
      </Message>
    )
  } else if (data.status === 'empty') {
    content = (
      <Message icon={ChartColumn} title={t('stats.emptyTitle')}>
        {t('stats.emptyText')}
      </Message>
    )
  } else {
    content = (
      <div className="space-y-5">
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Tile label={t('stats.total')} value={n(totals.alltorrents)} accent />
          <Tile label={t('stats.trackers')} value={n(totals.trackers)} />
          <Tile label={t('stats.newToday')} value={n(totals.newtor)} />
          <Tile label={t('stats.updatedToday')} value={n(totals.update)} />
        </div>

        <div className="card overflow-hidden">
          {data.rows.length > 6 && (
            <div className="border-b border-line p-3 sm:px-4">
              <label className="relative block max-w-xs">
                <span className="sr-only">{t('stats.filter')}</span>
                <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-faint" aria-hidden />
                <input
                  type="search"
                  className="input pl-9"
                  placeholder={t('stats.filter')}
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              </label>
            </div>
          )}
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="border-b border-line bg-surface-2/50 text-xs text-muted">
                <tr>
                  <SortHeader id="name" label={t('stats.col.tracker')} sort={sort} onSort={onSort} align="left" className="text-left" />
                  <SortHeader id="alltorrents" label={t('stats.col.alltorrents')} sort={sort} onSort={onSort} className="w-[38%] text-right" />
                  <SortHeader id="newtor" label={t('stats.col.newtor')} sort={sort} onSort={onSort} className="text-right" />
                  <SortHeader id="update" label={t('stats.col.update')} sort={sort} onSort={onSort} className="hidden text-right sm:table-cell" />
                  <SortHeader id="lastnewtor" label={t('stats.col.lastnewtor')} sort={sort} onSort={onSort} className="hidden text-right md:table-cell" />
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {rows.map((r) => {
                  const pct = barPercent(r.alltorrents, max)
                  const share = totals.alltorrents ? ((Number(r.alltorrents) || 0) / totals.alltorrents) * 100 : 0
                  return (
                    <tr key={r.trackerName} className="hover:bg-surface-2/40">
                      <th scope="row" className="px-3 py-3 text-left font-medium sm:px-4">
                        <div className="flex items-center gap-2.5">
                          <img src={trackerIcon(r.trackerName)} alt="" width="18" height="18" className="size-[18px] rounded-sm" loading="lazy" />
                          <div className="min-w-0">
                            <div className="truncate">{trackerLabel(r.trackerName)}</div>
                            <div className="text-xs font-normal text-faint md:hidden">{lastNewLabel(r.lastnewtor)}</div>
                          </div>
                        </div>
                      </th>
                      <td className="px-3 py-3 text-right sm:px-4">
                        <div className="font-medium tabular-nums">{n(r.alltorrents)}</div>
                        <div
                          className="mt-1.5 ml-auto h-1.5 w-full max-w-56 overflow-hidden rounded-full bg-surface-2"
                          role="img"
                          aria-label={`${t('stats.share')}: ${share.toFixed(1)}%`}
                          title={`${share.toFixed(1)}%`}
                        >
                          <div className="ml-auto h-full rounded-full bg-brand" style={{ width: `${pct}%` }} />
                        </div>
                      </td>
                      <td className="px-3 py-3 text-right tabular-nums sm:px-4">
                        {Number(r.newtor) > 0 ? <span className="font-medium text-ok">+{n(r.newtor)}</span> : <span className="text-faint">0</span>}
                      </td>
                      <td className="hidden px-3 py-3 text-right text-muted tabular-nums sm:table-cell sm:px-4">{n(r.update)}</td>
                      <td className="hidden px-3 py-3 text-right text-muted tabular-nums md:table-cell md:px-4">{lastNewLabel(r.lastnewtor)}</td>
                    </tr>
                  )
                })}
                {rows.length === 0 && (
                  <tr>
                    <td colSpan={5} className="px-4 py-8 text-center text-muted">
                      {t('stats.noMatch', { q: filter })}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    )
  }

  const computed = data.meta?.updatedAt ? formatDateTime(data.meta.updatedAt, locale) : ''

  return (
    <div className="mx-auto max-w-7xl px-4 pt-6 pb-16 sm:px-6 sm:pt-10">
      <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t('stats.title')}</h1>
          <p className="mt-1.5 text-sm text-muted">{t('stats.subtitle')}</p>
          {(data.lastDb || computed) && (
            <dl className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs text-muted">
              {data.lastDb && (
                <div className="flex gap-1.5">
                  <dt>{t('stats.lastDb')}:</dt>
                  <dd className="font-medium text-fg tabular-nums">{data.lastDb}</dd>
                </div>
              )}
              {computed && (
                <div className="flex gap-1.5">
                  <dt>{t('stats.computed')}:</dt>
                  <dd className="font-medium text-fg tabular-nums">{computed}</dd>
                </div>
              )}
            </dl>
          )}
        </div>
        <button
          type="button"
          className="btn btn-outline"
          onClick={() => {
            setData((d) => ({ ...d, status: d.status === 'done' ? 'done' : 'loading' }))
            setAttempt((a) => a + 1)
          }}
        >
          <RefreshCw className="size-4" aria-hidden />
          {t('stats.refresh')}
        </button>
      </div>
      {content}
    </div>
  )
}
