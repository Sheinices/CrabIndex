import { useState } from 'react'
import { Link } from 'react-router'
import { Pause, Play, RefreshCw, RotateCcw, Settings, XCircle } from 'lucide-react'
import { closeBrowserSessions, getCloudflareStatus, pauseCloudflare, resetCloudflareStats } from '../lib/api.js'
import { advice, avgSeconds, failRate, hostTone, kindLabel, totals } from '../lib/cloudflare.js'
import { usePolling } from '../hooks/usePolling.js'
import { useConfirm } from '../components/Confirm.jsx'
import { useToast } from '../components/Toast.jsx'
import { ErrorBox, PageHeader, Spinner, StatusDot, Toggle } from '../components/ui.jsx'
import { formatDate, formatNumber, formatRelative } from '../lib/format.js'
import { useT } from '../lang/index.jsx'

const TONE_DOT = { bad: 'danger', warn: 'warn', ok: 'ok', idle: 'muted' }
const ADVICE_STYLE = {
  danger: 'border-danger/40 bg-danger/10',
  warn: 'border-warn/40 bg-warn/10',
  info: 'border-border bg-surface-2',
}

function Stat({ label, value, sub, tone }) {
  return (
    <div className="card p-4">
      <div className="flex items-center gap-2 text-xs text-muted">
        {tone ? <StatusDot tone={tone} /> : null}
        {label}
      </div>
      <div className="mt-1 text-xl font-semibold tabular-nums">{value}</div>
      {sub ? <div className="mt-0.5 text-xs text-muted">{sub}</div> : null}
    </div>
  )
}

function Advice({ items }) {
  if (!items.length) return null
  return (
    <div className="mb-6 space-y-3">
      {items.map((a) => (
        <div key={a.title} role={a.tone === 'danger' ? 'alert' : 'status'} className={`rounded-xl border px-4 py-3 text-sm ${ADVICE_STYLE[a.tone]}`}>
          <div className="font-semibold">{a.title}</div>
          <p className="mt-1 text-muted">{a.text}</p>
          {a.command ? <code className="mt-2 block overflow-x-auto rounded-lg bg-bg px-3 py-2 font-mono text-xs">{a.command}</code> : null}
        </div>
      ))}
    </div>
  )
}

export function CloudflarePage() {
  const t = useT()
  const [live, setLive] = useState(true)
  const st = usePolling(() => getCloudflareStatus(), 10000, { enabled: live })
  const confirm = useConfirm()
  const toast = useToast()
  const [busy, setBusy] = useState('')

  const act = async (key, fn, done) => {
    setBusy(key)
    try {
      const r = await fn()
      toast.success(done(r))
    } catch (e) {
      if (e?.status !== 401) toast.error(t('error'), e?.message || String(e))
    } finally {
      setBusy('')
      st.reload()
    }
  }

  const closeAll = async () => {
    const ok = await confirm({
      title: t('cf_close_all_title'),
      message: t('cf_close_all_msg'),
      confirmLabel: t('close'),
    })
    if (ok)
      await act('close', () => closeBrowserSessions(), (r) => t('cf_closed_sessions', { closed: r.closed }) + (r.busy ? t('cf_busy_suffix', { busy: r.busy }) : ''))
  }

  const closeHost = (host) =>
    act(`close:${host}`, () => closeBrowserSessions(host), (r) => t('cf_host_closed', { host, closed: r.closed }) + (r.busy ? t('cf_host_busy_suffix') : ''))

  const togglePause = async () => {
    const pausing = !st.data?.paused
    if (pausing) {
      const ok = await confirm({
        title: t('cf_pause_title'),
        message: t('cf_pause_msg'),
        confirmLabel: t('cf_pause'),
        danger: true,
      })
      if (!ok) return
    }
    await act('pause', () => pauseCloudflare(pausing), (r) => (r.paused ? t('cf_paused_toast', { closed: r.closed }) : t('cf_resumed_toast')))
  }

  const resetStats = async () => {
    const ok = await confirm({ title: t('cf_reset_title'), message: t('cf_reset_msg'), confirmLabel: t('cf_reset_ok') })
    if (ok) await act('reset', () => resetCloudflareStats(), () => t('cf_reset_done'))
  }

  const d = st.data
  const hosts = d?.stats?.hosts || []
  const tot = totals(hosts)
  const errors = d?.stats?.recentErrors || []
  const sessions = d?.sessions || []
  const aliveSessions = sessions.filter((s) => s.alive).length
  const mode = !d ? '…' : !d.enabled ? t('cf_mode_off') : d.paused ? t('cf_mode_paused') : t('cf_mode_running')
  const modeTone = !d ? 'muted' : !d.enabled ? 'muted' : d.paused ? 'warn' : 'ok'

  return (
    <>
      <PageHeader
        title="FlareSolverr"
        description={t('cf_desc')}
        actions={
          <>
            <Toggle id="cf-live" checked={live} onChange={setLive} label={t('auto_refresh')} />
            <button type="button" className="btn btn-sm" onClick={st.reload}>
              <RefreshCw className="size-4" aria-hidden="true" /> {t('refresh')}
            </button>
            <button type="button" className="btn btn-sm" onClick={closeAll} disabled={!!busy || !d?.enabled}>
              {busy === 'close' ? <Spinner /> : <XCircle className="size-4" aria-hidden="true" />}
              {t('cf_close_sessions')}
            </button>
            <button type="button" className={`btn btn-sm ${d?.paused ? 'btn-primary' : 'btn-danger'}`} onClick={togglePause} disabled={!!busy || !d?.enabled}>
              {busy === 'pause' ? <Spinner /> : d?.paused ? <Play className="size-4" aria-hidden="true" /> : <Pause className="size-4" aria-hidden="true" />}
              {d?.paused ? t('cf_resume') : t('cf_pause')}
            </button>
          </>
        }
      />
      <ErrorBox error={st.error} onRetry={st.reload} />
      {st.loading && !d ? (
        <Spinner label={t('loading')} />
      ) : d ? (
        <>
          <div className="mb-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
            <Stat
              label="FlareSolverr"
              tone={!d.enabled ? 'muted' : d.solver?.reachable ? 'ok' : 'danger'}
              value={!d.enabled ? t('cf_solver_off') : d.solver?.reachable ? `v${d.solver.version || '?'}` : t('cf_solver_unreachable')}
              sub={d.solver?.reachable ? t('cf_solver_sessions', { n: (d.solver.sessions || []).length }) : d.solver?.error || d.settings?.url}
            />
            <Stat label={t('cf_mode')} tone={modeTone} value={mode} sub={t('cf_active_sessions', { n: aliveSessions })} />
            <Stat
              label={t('cf_browser_requests')}
              tone={tot.browserFailed ? (tot.tabCrashed ? 'danger' : 'warn') : 'muted'}
              value={`${formatNumber(tot.browserOk)} / ${formatNumber(tot.browserRequests)}`}
              sub={t('cf_errors_count', { n: formatNumber(tot.browserFailed) }) + (tot.tabCrashed ? t('cf_tab_crashed_suffix', { n: formatNumber(tot.tabCrashed) }) : '')}
            />
            <Stat
              label={t('cf_fastpath')}
              tone={d.cffetch?.enabled ? 'ok' : 'muted'}
              value={d.cffetch?.enabled ? t('cf_fast_ok', { n: formatNumber(tot.fastOk) }) : t('cf_off')}
              sub={d.cffetch?.enabled ? t('cf_fast_sub', { failed: formatNumber(tot.fastFailed), sites: (d.cffetch.hosts || []).filter((h) => h.clearanceAt).length }) : null}
            />
          </div>

          <Advice items={advice(d)} />

          <section className="card mb-6 p-5" aria-labelledby="cf-hosts">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
              <h2 id="cf-hosts" className="font-semibold">
                {t('cf_hosts_title')}
              </h2>
              <span className="text-xs text-muted">{t('cf_since', { date: formatDate(d.stats?.since) })}</span>
            </div>
            {hosts.length ? (
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>{t('cf_col_site')}</th>
                      <th className="text-right">{t('cf_col_browser')}</th>
                      <th className="text-right">{t('cf_col_errors')}</th>
                      <th>{t('cf_col_causes')}</th>
                      <th className="text-right">{t('cf_col_avg')}</th>
                      <th className="text-right">{t('cf_col_sessions')}</th>
                      <th className="text-right">{t('cf_col_cffetch')}</th>
                      <th>{t('cf_col_last_error')}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {hosts.map((h) => {
                      const causes = [
                        ['tabCrashed', h.tabCrashed],
                        ['browserTimeout', h.browserTimeouts],
                        ['challengeFailed', h.challengeFailed],
                        ['sessionError', h.sessionErrors],
                        ['unreachable', h.unreachable],
                        ['pageFailed', h.pageFailed],
                        ['other', h.otherErrors],
                      ].filter(([, n]) => n > 0)
                      const rate = failRate(h)
                      return (
                        <tr key={h.host}>
                          <td>
                            <span className="flex items-center gap-2 font-medium">
                              <StatusDot tone={TONE_DOT[hostTone(h)]} />
                              {h.host}
                            </span>
                          </td>
                          <td className="text-right tabular-nums">{formatNumber(h.browserRequests)}</td>
                          <td className="text-right tabular-nums">
                            {formatNumber(h.browserFailed)}
                            {rate ? <span className="text-xs text-muted"> ({rate}%)</span> : null}
                          </td>
                          <td>
                            <div className="flex flex-wrap gap-1">
                              {causes.length ? causes.map(([k, n]) => <span key={k} className="badge">{`${kindLabel(k)}: ${n}`}</span>) : <span className="text-xs text-muted">{t('cf_none')}</span>}
                            </div>
                          </td>
                          <td className="text-right tabular-nums">{avgSeconds(h) ?? '-'}</td>
                          <td className="text-right tabular-nums" title="создано / пересоздано / закрыто по простою">
                            {h.sessionsCreated} / {h.sessionsRecycled} / {h.sessionsClosedIdle}
                          </td>
                          <td className="text-right tabular-nums" title="быстрый путь: сработал / нет, обновлений cookie">
                            {formatNumber(h.fastOk)} / {formatNumber(h.fastFailed)}
                          </td>
                          <td className="max-w-xs">
                            {h.lastErrorAt ? (
                              <span className="block truncate text-xs" title={h.lastError}>
                                {formatRelative(h.lastErrorAt)}: {h.lastError}
                              </span>
                            ) : (
                              <span className="text-xs text-muted">-</span>
                            )}
                          </td>
                          <td className="text-right">
                            <button type="button" className="btn btn-sm btn-ghost" onClick={() => closeHost(h.host)} disabled={!!busy} aria-label={t('cf_close_session_host', { host: h.host })} title={t('cf_close_session_title')}>
                              {busy === `close:${h.host}` ? <Spinner /> : <XCircle className="size-4" aria-hidden="true" />}
                            </button>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-sm text-muted">{t('cf_no_browser_requests')}</p>
            )}
          </section>

          <div className="mb-6 grid gap-6 lg:grid-cols-2">
            <section className="card p-5" aria-labelledby="cf-errors">
              <h2 id="cf-errors" className="mb-4 font-semibold">
                {t('cf_recent_errors')}
              </h2>
              {errors.length ? (
                <ul className="max-h-96 space-y-2 overflow-y-auto text-sm">
                  {errors.map((e, i) => (
                    <li key={`${e.at}-${i}`} className="rounded-lg border border-border px-3 py-2">
                      <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
                        <span title={formatDate(e.at)}>{formatRelative(e.at)}</span>
                        <span className="font-medium text-fg">{e.host}</span>
                        <span className="badge">{kindLabel(e.kind)}</span>
                      </div>
                      <div className="mt-1 break-words text-xs">{e.message}</div>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="text-sm text-muted">{t('cf_no_errors')}</p>
              )}
            </section>

            <section className="card p-5" aria-labelledby="cf-sessions">
              <h2 id="cf-sessions" className="mb-4 font-semibold">
                {t('cf_sessions_title')}
              </h2>
              {sessions.length ? (
                <ul className="space-y-2 text-sm">
                  {sessions.map((s) => (
                    <li key={s.name} className="flex items-center justify-between gap-2">
                      <span className="flex items-center gap-2">
                        <StatusDot tone={s.busy ? 'brand' : s.alive ? 'ok' : 'muted'} />
                        <span className="font-medium">{s.host}</span>
                      </span>
                      <span className="text-xs text-muted">
                        {s.busy ? t('cf_session_busy') : s.alive ? t('cf_session_open') : t('cf_session_closed')}
                        {s.lastUse ? ` · ${formatRelative(s.lastUse)}` : ''}
                        {s.consecutiveTimeouts ? t('cf_session_timeouts', { n: s.consecutiveTimeouts }) : ''}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="text-sm text-muted">{t('cf_no_sessions')}</p>
              )}
              <p className="mt-4 text-xs text-muted">{t('cf_sessions_note', { n: d.settings?.sessionIdleMinutes })}</p>
            </section>
          </div>

          <section className="card p-5" aria-labelledby="cf-settings">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
              <h2 id="cf-settings" className="font-semibold">
                {t('cf_settings_title')}
              </h2>
              <div className="flex gap-2">
                <button type="button" className="btn btn-sm" onClick={resetStats} disabled={!!busy}>
                  <RotateCcw className="size-4" aria-hidden="true" /> {t('cf_reset_stats')}
                </button>
                <Link to="/settings" className="btn btn-sm">
                  <Settings className="size-4" aria-hidden="true" /> {t('cf_edit_settings')}
                </Link>
              </div>
            </div>
            <dl className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
              {[
                [t('cf_s_url'), d.settings?.url],
                [t('cf_s_crawlurl'), d.settings?.crawlUrl || t('cf_none')],
                [t('cf_s_timeout'), t('cf_seconds', { n: Math.round((d.settings?.maxTimeoutMs || 0) / 1000) })],
                [t('cf_s_idle'), t('cf_minutes', { n: d.settings?.sessionIdleMinutes })],
                [t('cf_s_recycle'), d.settings?.recycleAfterTimeouts],
                [t('cf_s_guarded_hours'), t('cf_hours', { n: d.settings?.guardedHours })],
                ['cffetch', d.cffetch?.enabled ? `${d.cffetch.url} · ${d.cffetch.impersonate}` : t('cf_off')],
                [t('cf_s_guarded_hosts'), (d.guarded || []).map((g) => g.host).join(', ') || t('cf_none')],
              ].map(([k, v]) => (
                <div key={k} className="flex justify-between gap-3 border-b border-border py-1.5">
                  <dt className="text-muted">{k}</dt>
                  <dd className="text-right font-medium break-all">{String(v ?? '-')}</dd>
                </div>
              ))}
            </dl>
            <p className="mt-4 text-xs text-muted">{t('cf_limits_note')}</p>
            <code className="mt-2 block overflow-x-auto rounded-lg bg-bg px-3 py-2 font-mono text-xs">docker update --cpus 2 --memory 4g --memory-swap 4g flaresolverr</code>
          </section>
        </>
      ) : null}
    </>
  )
}
