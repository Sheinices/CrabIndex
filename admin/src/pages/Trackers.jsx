// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useId, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Download, FileText, KeyRound, Play, RefreshCw, Search } from 'lucide-react'
import { checkTrackerLogin, getOverview, getStatsHistory, runCron } from '../lib/api.js'
import { actionsFor, cleanParams, hasParseAll, TRACKER_ACTIONS, trackerLogName } from '../lib/actions.js'
import { usePolling } from '../hooks/usePolling.js'
import { useResult } from '../components/ResultDrawer.jsx'
import { useConfirm } from '../components/Confirm.jsx'
import { Modal } from '../components/Modal.jsx'
import { ErrorBox, PageHeader, ProgressBar, Spinner, StatusDot } from '../components/ui.jsx'
import { formatNumber } from '../lib/format.js'
import { useT } from '../lang/index.jsx'

export function parseAllProgress(pa) {
  if (!pa) return null
  const total = Number(pa.mapCount)
  const pending = Number(pa.pending)
  if (!(total > 0) || !Number.isFinite(pending)) return null
  return Math.max(0, Math.min(100, Math.round(((total - pending) / total) * 100)))
}

function ParamsDialog({ state, onClose, onRun }) {
  const t = useT()
  const [values, setValues] = useState({})
  const formId = useId()
  if (!state) return null
  const { tracker, action } = state
  return (
    <Modal
      open
      onClose={onClose}
      title={`${tracker.name || tracker.slug}: ${action.label}`}
      description={action.description}
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="submit" form={formId} className="btn btn-primary">
            <Play className="size-4" aria-hidden="true" /> {t('run')}
          </button>
        </>
      }
    >
      <form
        id={formId}
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault()
          onRun(cleanParams(values))
        }}
      >
        {action.params.map((p, i) => (
          <div key={p.name}>
            <label className="label" htmlFor={`${formId}-${p.name}`}>
              {p.label} <span className="font-mono">({p.name})</span>
            </label>
            <input
              id={`${formId}-${p.name}`}
              className="input"
              type={p.type === 'number' ? 'number' : 'text'}
              inputMode={p.type === 'number' ? 'numeric' : undefined}
              placeholder={p.placeholder}
              value={values[p.name] ?? ''}
              data-autofocus={i === 0 ? true : undefined}
              onChange={(e) => setValues((v) => ({ ...v, [p.name]: e.target.value }))}
            />
          </div>
        ))}
        <p className="text-xs text-muted">{t('tr_empty_defaults')}</p>
      </form>
    </Modal>
  )
}

/** Login state of a tracker that needs an account, with the on-demand check button. */
/// New torrents per day for one tracker over the last month: thin bars, one hue, the day and
/// value on hover; the total and the best day as text so the number is never color-alone.
function FillSparkline({ days, values, t }) {
  const nums = (values || []).map((v) => (Number.isFinite(Number(v)) ? Number(v) : null))
  const known = nums.filter((v) => v != null)
  if (!days?.length || known.length === 0) return <span className="text-xs text-muted">{t('tr_fill_none')}</span>
  const max = Math.max(1, ...known)
  const total = known.reduce((a, b) => a + b, 0)
  const w = 3
  const gap = 2
  const h = 22
  const width = days.length * (w + gap) - gap
  return (
    <div className="flex items-center gap-2">
      <svg width={width} height={h} viewBox={`0 0 ${width} ${h}`} role="img" aria-label={t('tr_fill_aria', { total: formatNumber(total), days: days.length })} className="shrink-0 text-accent">
        <line x1="0" y1={h - 0.5} x2={width} y2={h - 0.5} stroke="currentColor" strokeOpacity="0.25" strokeWidth="1" />
        {nums.map((v, i) => {
          const x = i * (w + gap)
          if (v == null) return <rect key={days[i]} x={x} y={h - 2} width={w} height={1} fill="currentColor" fillOpacity="0.2" />
          const bh = Math.max(1, Math.round((v / max) * (h - 2)))
          return (
            <rect key={days[i]} x={x} y={h - 1 - bh} width={w} height={bh} rx="1" fill="currentColor" fillOpacity={v === max ? 1 : 0.7}>
              <title>{`${days[i]}: ${formatNumber(v)}`}</title>
            </rect>
          )
        })}
      </svg>
      <span className="text-xs tabular-nums whitespace-nowrap text-muted" title={t('tr_fill_max', { n: formatNumber(max) })}>
        {formatNumber(total)}
      </span>
    </div>
  )
}

function LoginCell({ tracker, busy, onCheck }) {
  const t = useT()
  const l = tracker.login
  if (!l || !l.required) return <span className="text-xs text-muted">{t('tr_login_not_required')}</span>
  let tone = 'warn'
  let label = t('tr_login_missing')
  let title = ''
  if (l.configured) {
    if (!l.status) {
      tone = 'muted'
      label = t('tr_login_unchecked')
    } else if (l.status.ok) {
      tone = 'ok'
      label = t('tr_login_ok')
      title = l.status.at
    } else {
      tone = 'danger'
      label = t('tr_login_failed')
      title = l.status.error
    }
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span title={title}>
        <StatusDot tone={tone} label={label} />
      </span>
      {l.canCheck && l.configured ? (
        <button type="button" className="btn btn-sm btn-ghost" onClick={onCheck} disabled={busy} title={t('tr_login_check_hint')}>
          {busy ? <Spinner className="size-3.5" /> : <KeyRound className="size-3.5" aria-hidden="true" />} {t('tr_login_check')}
        </button>
      ) : null}
      {!l.configured ? (
        <Link to="/settings" className="text-xs text-muted hover:text-fg">
          {t('hint_open_settings')}
        </Link>
      ) : null}
    </div>
  )
}

export function TrackersPage() {
  const t = useT()
  const { data, error, loading, reload } = usePolling(() => getOverview(), 15_000)
  const history = usePolling(() => getStatsHistory(30), 5 * 60_000)
  const hist = history.data && typeof history.data === 'object' ? history.data : null
  const { run } = useResult()
  const confirm = useConfirm()
  const [query, setQuery] = useState('')
  const [busy, setBusy] = useState(null)
  const [params, setParams] = useState(null)

  const trackers = useMemo(() => {
    const list = data?.trackers?.length
      ? data.trackers
      : Object.keys(TRACKER_ACTIONS).map((slug) => ({ slug, name: slug, enabled: true, parseAll: null }))
    const q = query.trim().toLowerCase()
    return [...list]
      .filter((t) => !q || t.slug.toLowerCase().includes(q) || String(t.name || '').toLowerCase().includes(q))
      .sort((a, b) => a.slug.localeCompare(b.slug))
  }, [data, query])

  const execute = async (tracker, action, query) => {
    const key = `${tracker.slug}:${action.id}`
    setBusy(key)
    await run(`${tracker.name || tracker.slug}: ${action.label}`, () => runCron(tracker.slug, action.id, query))
    setBusy(null)
  }

  const checkLogin = async (tracker) => {
    setBusy(`${tracker.slug}:login`)
    await run(t('tr_check_login_of', { slug: tracker.slug }), () => checkTrackerLogin(tracker.slug))
    setBusy(null)
    reload()
    reload()
  }

  const start = async (tracker, action) => {
    if (action.params.length) {
      setParams({ tracker, action })
      return
    }
    if (action.heavy) {
      const ok = await confirm({
        title: `${action.label} - ${tracker.name || tracker.slug}`,
        message: <p>{t('tr_heavy_confirm', { desc: action.description })}</p>,
        confirmLabel: t('run'),
      })
      if (!ok) return
    }
    execute(tracker, action)
  }

  return (
    <>
      <PageHeader
        title={t('nav_trackers')}
        description={t('tr_desc')}
        actions={
          <>
            <a href="/stats/torrents?format=csv" className="btn btn-sm" download title={t('tr_csv_hint')}>
              <Download className="size-4" aria-hidden="true" /> CSV
            </a>
            <button type="button" className="btn btn-sm" onClick={reload}>
              <RefreshCw className="size-4" aria-hidden="true" /> {t('refresh')}
            </button>
          </>
        }
      />
      <ErrorBox error={error} onRetry={reload} />
      <div className="relative mb-4 max-w-sm">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
        <input
          type="search"
          className="input pl-9"
          placeholder={t('tr_search')}
          aria-label={t('tr_search')}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      {loading && !data ? (
        <Spinner className="size-5" label={t('loading')} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th scope="col">{t('tr_col_tracker')}</th>
                <th scope="col">{t('status')}</th>
                <th scope="col">{t('tr_col_login')}</th>
                <th scope="col" className="min-w-44">
                  ParseAll
                </th>
                <th scope="col">{t('tr_col_fill')}</th>
                <th scope="col">{t('tr_col_actions')}</th>
              </tr>
            </thead>
            <tbody>
              {trackers.map((tr) => {
                const pa = tr.parseAll
                const pct = parseAllProgress(pa)
                return (
                  <tr key={tr.slug}>
                    <td>
                      <div className="font-medium">{tr.name || tr.slug}</div>
                      <div className="font-mono text-xs text-muted">{tr.slug}</div>
                    </td>
                    <td className="text-sm whitespace-nowrap">
                      <StatusDot tone={tr.enabled === false ? 'muted' : 'ok'} label={tr.enabled === false ? t('tr_off') : t('tr_on')} />
                    </td>
                    <td className="text-sm">
                      <LoginCell tracker={tr} busy={busy === `${tr.slug}:login`} onCheck={() => checkLogin(tr)} />
                    </td>
                    <td>
                      {hasParseAll(tr.slug) && pa ? (
                        <div className="space-y-1">
                          <ProgressBar value={pa.running && pct == null ? null : (pct ?? 0)} label={`ParseAll ${tr.slug}`} />
                          <p className="text-xs text-muted tabular-nums">
                            {pa.running ? t('tr_running_prefix') : ''}
                            {t('tr_remaining', { pending: formatNumber(pa.pending), total: formatNumber(pa.mapCount) })}
                          </p>
                        </div>
                      ) : (
                        <span className="text-xs text-muted">-</span>
                      )}
                    </td>
                    <td>
                      <FillSparkline days={hist?.days} values={hist?.trackers?.[tr.slug]?.new} t={t} />
                    </td>
                    <td>
                      <div className="flex flex-wrap gap-1.5">
                        {actionsFor(tr.slug).map((a) => (
                          <button
                            key={a.id}
                            type="button"
                            className="btn btn-sm"
                            title={a.description}
                            disabled={busy === `${tr.slug}:${a.id}`}
                            onClick={() => start(tr, a)}
                          >
                            {busy === `${tr.slug}:${a.id}` ? <Spinner className="size-3.5" /> : null}
                            {a.label}
                          </button>
                        ))}
                        <Link to={`/logs?name=${encodeURIComponent(trackerLogName(tr.slug))}`} className="btn btn-sm btn-ghost" aria-label={t('tr_log_of', { slug: tr.slug })}>
                          <FileText className="size-3.5" aria-hidden="true" /> {t('tr_log')}
                        </Link>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
      <ParamsDialog
        key={params ? `${params.tracker.slug}:${params.action.id}` : 'none'}
        state={params}
        onClose={() => setParams(null)}
        onRun={(q) => {
          const p = params
          setParams(null)
          execute(p.tracker, p.action, q)
        }}
      />
    </>
  )
}
