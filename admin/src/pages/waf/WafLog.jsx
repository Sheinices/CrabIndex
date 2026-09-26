import { useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { Filter, Pause, Play, RefreshCw, X } from 'lucide-react'
import { getWafRequests } from '../../lib/api.js'
import { usePolling } from '../../hooks/usePolling.js'
import { ErrorBox, Spinner, Toggle } from '../../components/ui.jsx'
import { buildRequestsQuery, formatTime } from '../../lib/waf.js'
import { formatDate } from '../../lib/format.js'
import { Empty, ReasonBadge, StatusBadge } from './shared.jsx'
import { useT } from '../../lang/index.jsx'

// The first option's label is an i18n key (resolved in the component); the rest are literal codes.
const STATUS_OPTIONS = [
  ['', 'waf_all_statuses'],
  ['2xx', '2xx'],
  ['3xx', '3xx'],
  ['4xx', '4xx'],
  ['5xx', '5xx'],
  ['403', '403'],
  ['404', '404'],
  ['429', '429'],
]
const LIMITS = [100, 200, 500, 1000]

function readFilters(params) {
  return {
    ip: params.get('ip') || '',
    path: params.get('path') || '',
    origin: params.get('origin') || '',
    host: params.get('host') || '',
    status: params.get('status') || '',
    blocked: params.get('blocked') === '1',
    limit: Number(params.get('limit')) || 200,
  }
}

export function WafLog() {
  const t = useT()
  const [params, setParams] = useSearchParams()
  const filters = useMemo(() => readFilters(params), [params])
  const [draft, setDraft] = useState({ ip: filters.ip, path: filters.path, origin: filters.origin, host: filters.host })
  const [paused, setPaused] = useState(false)
  const query = useMemo(() => buildRequestsQuery(filters), [filters])
  const { data, error, loading, reload } = usePolling(() => getWafRequests(query), paused ? 0 : 5000, { enabled: !paused })

  // Keep the text inputs in sync when filters change from outside (IP click, links).
  useEffect(() => {
    setDraft({ ip: filters.ip, path: filters.path, origin: filters.origin, host: filters.host })
  }, [filters.ip, filters.path, filters.origin, filters.host])

  useEffect(() => {
    reload()
  }, [query, reload])

  const update = (patch) => {
    const next = { ...filters, ...patch }
    const p = new URLSearchParams()
    if (next.ip) p.set('ip', next.ip)
    if (next.path) p.set('path', next.path)
    if (next.origin) p.set('origin', next.origin)
    if (next.host) p.set('host', next.host)
    if (next.status) p.set('status', next.status)
    if (next.blocked) p.set('blocked', '1')
    if (next.limit && next.limit !== 200) p.set('limit', String(next.limit))
    setParams(p)
  }

  const rows = Array.isArray(data) ? data : Array.isArray(data?.requests) ? data.requests : []
  const active = filters.ip || filters.path || filters.origin || filters.host || filters.status || filters.blocked

  return (
    <div className="space-y-4">
      <form
        className="card flex flex-wrap items-end gap-3 p-4"
        aria-label={t('waf_filters')}
        onSubmit={(e) => {
          e.preventDefault()
          update({ ip: draft.ip.trim(), path: draft.path.trim(), origin: draft.origin.trim(), host: draft.host.trim() })
        }}
      >
        <div className="w-full sm:w-44">
          <label className="label" htmlFor="waf-f-ip">
            IP
          </label>
          <input id="waf-f-ip" className="input font-mono" value={draft.ip} onChange={(e) => setDraft((d) => ({ ...d, ip: e.target.value }))} placeholder="203.0.113.7" spellCheck={false} />
        </div>
        <div className="min-w-48 flex-1">
          <label className="label" htmlFor="waf-f-path">
            {t('waf_path_contains')}
          </label>
          <input id="waf-f-path" className="input font-mono" value={draft.path} onChange={(e) => setDraft((d) => ({ ...d, path: e.target.value }))} placeholder="/api/v2.0/indexers" spellCheck={false} />
        </div>
        <div className="w-full sm:w-44">
          <label className="label" htmlFor="waf-f-origin">
            {t('waf_domain_origin')}
          </label>
          <input
            id="waf-f-origin"
            className="input font-mono"
            value={draft.origin}
            onChange={(e) => setDraft((d) => ({ ...d, origin: e.target.value }))}
            placeholder="example.com"
            spellCheck={false}
          />
        </div>
        <div className="w-full sm:w-44">
          <label className="label" htmlFor="waf-f-host">
            {t('waf_host_host')}
          </label>
          <input
            id="waf-f-host"
            className="input font-mono"
            value={draft.host}
            onChange={(e) => setDraft((d) => ({ ...d, host: e.target.value }))}
            placeholder="sync.example.com"
            spellCheck={false}
          />
        </div>
        <div className="w-full sm:w-36">
          <label className="label" htmlFor="waf-f-status">
            {t('status')}
          </label>
          <select id="waf-f-status" className="input" value={filters.status} onChange={(e) => update({ status: e.target.value })}>
            {STATUS_OPTIONS.map(([v, l]) => (
              <option key={v} value={v}>
                {v === '' ? t(l) : l}
              </option>
            ))}
          </select>
        </div>
        <div className="w-full sm:w-28">
          <label className="label" htmlFor="waf-f-limit">
            {t('logs_lines')}
          </label>
          <select id="waf-f-limit" className="input" value={filters.limit} onChange={(e) => update({ limit: Number(e.target.value) })}>
            {LIMITS.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </div>
        <div className="pb-2">
          <Toggle id="waf-f-blocked" checked={filters.blocked} onChange={(v) => update({ blocked: v })} label={t('only_blocked')} />
        </div>
        <div className="flex gap-2">
          <button type="submit" className="btn btn-primary">
            <Filter className="size-4" aria-hidden="true" /> {t('apply')}
          </button>
          {active ? (
            <button type="button" className="btn" onClick={() => setParams(new URLSearchParams())}>
              <X className="size-4" aria-hidden="true" /> {t('reset')}
            </button>
          ) : null}
        </div>
      </form>

      <div className="flex flex-wrap items-center gap-3">
        <p className="text-sm text-muted" aria-live="polite">
          {loading && !data ? t('loading') : t('waf_shown_newest', { n: rows.length })}
          {paused ? t('waf_paused') : t('waf_every5')}
        </p>
        <div className="ml-auto flex gap-2">
          <button type="button" className="btn btn-sm" onClick={() => setPaused((p) => !p)} aria-pressed={paused}>
            {paused ? <Play className="size-4" aria-hidden="true" /> : <Pause className="size-4" aria-hidden="true" />}
            {paused ? t('resume') : t('pause')}
          </button>
          <button type="button" className="btn btn-sm" onClick={reload} aria-label={t('waf_refresh_log')}>
            <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
          </button>
        </div>
      </div>

      <ErrorBox error={error} onRetry={reload} />
      {loading && !data ? (
        <Spinner className="size-5" label={t('loading')} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>{t('waf_time')}</th>
                <th>IP</th>
                <th>{t('waf_host')}</th>
                <th>{t('waf_request')}</th>
                <th>Origin</th>
                <th>{t('status')}</th>
                <th className="text-right">{t('waf_ms')}</th>
                <th>{t('waf_block_col')}</th>
                <th>User-Agent</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={`${r.time}-${r.ip}-${i}`} className={r.blocked ? 'bg-danger/5' : undefined}>
                  <td className="text-xs whitespace-nowrap text-muted tabular-nums" title={formatDate(r.time)}>
                    {formatTime(r.time)}
                  </td>
                  <td>
                    <button
                      type="button"
                      className="font-mono text-xs text-accent hover:underline"
                      onClick={() => update({ ip: r.ip })}
                      title={t('waf_show_ip_reqs')}
                    >
                      {r.ip}
                    </button>
                  </td>
                  <td className="max-w-40">
                    {r.host && r.host !== '-' ? (
                      <button
                        type="button"
                        className="block max-w-full truncate font-mono text-xs text-accent hover:underline"
                        onClick={() => update({ host: r.host })}
                        title={t('waf_show_host_reqs', { host: r.host })}
                      >
                        {r.host}
                      </button>
                    ) : (
                      <span className="text-xs text-muted">-</span>
                    )}
                  </td>
                  <td className="max-w-md">
                    <span className="mr-1.5 font-mono text-[11px] font-semibold text-muted">{r.method}</span>
                    <span className="font-mono text-xs break-all">{r.path}</span>
                  </td>
                  <td className="max-w-48">
                    {r.origin ? (
                      <button
                        type="button"
                        className="block max-w-full truncate font-mono text-xs text-accent hover:underline"
                        onClick={() => update({ origin: r.origin })}
                        title={t('waf_show_origin_reqs', { origin: r.origin })}
                      >
                        {r.origin}
                      </button>
                    ) : (
                      <span className="text-xs text-muted">-</span>
                    )}
                  </td>
                  <td>
                    <StatusBadge status={r.status} />
                  </td>
                  <td className="text-right text-xs text-muted tabular-nums">{r.ms ?? '-'}</td>
                  <td>
                    <ReasonBadge reason={r.blocked} />
                  </td>
                  <td className="max-w-56 truncate text-xs text-muted" title={r.ua}>
                    {r.ua || '-'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!rows.length ? <Empty>{active ? t('waf_no_filtered') : t('waf_log_empty')}</Empty> : null}
        </div>
      )}
    </div>
  )
}
