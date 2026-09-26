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
      if (e?.status !== 401) toast.error('Ошибка', e?.message || String(e))
    } finally {
      setBusy('')
      st.reload()
    }
  }

  const closeAll = async () => {
    const ok = await confirm({
      title: 'Закрыть все сессии браузера?',
      message: 'Chrome во FlareSolverr освободит память. Следующий запрос к трекеру за Cloudflare заново пройдёт проверку (10-60 секунд). Сессии, занятые запросом, останутся.',
      confirmLabel: 'Закрыть',
    })
    if (ok) await act('close', () => closeBrowserSessions(), (r) => `Закрыто сессий: ${r.closed}${r.busy ? `, заняты: ${r.busy}` : ''}`)
  }

  const closeHost = (host) => act(`close:${host}`, () => closeBrowserSessions(host), (r) => `${host}: закрыто ${r.closed}${r.busy ? `, занята запросом` : ''}`)

  const togglePause = async () => {
    const pausing = !st.data?.paused
    if (pausing) {
      const ok = await confirm({
        title: 'Приостановить FlareSolverr?',
        message:
          'CrabIndex перестанет обращаться к браузеру и закроет все его сессии. Трекеры за Cloudflare не будут парситься, пока вы не включите его снова или не перезапустите службу. Настройки не меняются.',
        confirmLabel: 'Приостановить',
        danger: true,
      })
      if (!ok) return
    }
    await act('pause', () => pauseCloudflare(pausing), (r) => (r.paused ? `FlareSolverr приостановлен, закрыто сессий: ${r.closed}` : 'FlareSolverr снова работает'))
  }

  const resetStats = async () => {
    const ok = await confirm({ title: 'Сбросить статистику?', message: 'Счётчики и журнал ошибок обнулятся. Сессии браузера не затрагиваются.', confirmLabel: 'Сбросить' })
    if (ok) await act('reset', () => resetCloudflareStats(), () => 'Статистика сброшена')
  }

  const d = st.data
  const hosts = d?.stats?.hosts || []
  const t = totals(hosts)
  const errors = d?.stats?.recentErrors || []
  const sessions = d?.sessions || []
  const aliveSessions = sessions.filter((s) => s.alive).length
  const mode = !d ? '…' : !d.enabled ? 'выключен' : d.paused ? 'приостановлен' : 'работает'
  const modeTone = !d ? 'muted' : !d.enabled ? 'muted' : d.paused ? 'warn' : 'ok'

  return (
    <>
      <PageHeader
        title="FlareSolverr"
        description="Обход Cloudflare: состояние браузера, ошибки по трекерам и управление"
        actions={
          <>
            <Toggle id="cf-live" checked={live} onChange={setLive} label="Автообновление" />
            <button type="button" className="btn btn-sm" onClick={st.reload}>
              <RefreshCw className="size-4" aria-hidden="true" /> Обновить
            </button>
            <button type="button" className="btn btn-sm" onClick={closeAll} disabled={!!busy || !d?.enabled}>
              {busy === 'close' ? <Spinner /> : <XCircle className="size-4" aria-hidden="true" />}
              Закрыть сессии
            </button>
            <button type="button" className={`btn btn-sm ${d?.paused ? 'btn-primary' : 'btn-danger'}`} onClick={togglePause} disabled={!!busy || !d?.enabled}>
              {busy === 'pause' ? <Spinner /> : d?.paused ? <Play className="size-4" aria-hidden="true" /> : <Pause className="size-4" aria-hidden="true" />}
              {d?.paused ? 'Включить' : 'Приостановить'}
            </button>
          </>
        }
      />
      <ErrorBox error={st.error} onRetry={st.reload} />
      {st.loading && !d ? (
        <Spinner label="Загрузка…" />
      ) : d ? (
        <>
          <div className="mb-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
            <Stat
              label="FlareSolverr"
              tone={!d.enabled ? 'muted' : d.solver?.reachable ? 'ok' : 'danger'}
              value={!d.enabled ? 'выключен в настройках' : d.solver?.reachable ? `v${d.solver.version || '?'}` : 'не отвечает'}
              sub={d.solver?.reachable ? `сессий в браузере: ${(d.solver.sessions || []).length}` : d.solver?.error || d.settings?.url}
            />
            <Stat label="Режим" tone={modeTone} value={mode} sub={`активных сессий: ${aliveSessions}`} />
            <Stat
              label="Запросы через браузер"
              tone={t.browserFailed ? (t.tabCrashed ? 'danger' : 'warn') : 'muted'}
              value={`${formatNumber(t.browserOk)} / ${formatNumber(t.browserRequests)}`}
              sub={`ошибок: ${formatNumber(t.browserFailed)}${t.tabCrashed ? `, из них падений вкладки: ${formatNumber(t.tabCrashed)}` : ''}`}
            />
            <Stat
              label="Быстрый путь (cffetch)"
              tone={d.cffetch?.enabled ? 'ok' : 'muted'}
              value={d.cffetch?.enabled ? `${formatNumber(t.fastOk)} ок` : 'выключен'}
              sub={d.cffetch?.enabled ? `не сработал: ${formatNumber(t.fastFailed)} · с cookie: ${(d.cffetch.hosts || []).filter((h) => h.clearanceAt).length} сайт(ов)` : null}
            />
          </div>

          <Advice items={advice(d)} />

          <section className="card mb-6 p-5" aria-labelledby="cf-hosts">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
              <h2 id="cf-hosts" className="font-semibold">
                Трекеры за Cloudflare
              </h2>
              <span className="text-xs text-muted">с {formatDate(d.stats?.since)}</span>
            </div>
            {hosts.length ? (
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>Сайт</th>
                      <th className="text-right">Браузер</th>
                      <th className="text-right">Ошибки</th>
                      <th>Причины</th>
                      <th className="text-right">Среднее, с</th>
                      <th className="text-right">Сессий</th>
                      <th className="text-right">cffetch</th>
                      <th>Последняя ошибка</th>
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
                              {causes.length ? causes.map(([k, n]) => <span key={k} className="badge">{`${kindLabel(k)}: ${n}`}</span>) : <span className="text-xs text-muted">нет</span>}
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
                            <button type="button" className="btn btn-sm btn-ghost" onClick={() => closeHost(h.host)} disabled={!!busy} aria-label={`Закрыть сессию ${h.host}`} title="Закрыть сессию браузера этого сайта">
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
              <p className="text-sm text-muted">Пока ни одного запроса через браузер.</p>
            )}
          </section>

          <div className="mb-6 grid gap-6 lg:grid-cols-2">
            <section className="card p-5" aria-labelledby="cf-errors">
              <h2 id="cf-errors" className="mb-4 font-semibold">
                Последние ошибки
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
                <p className="text-sm text-muted">Ошибок нет.</p>
              )}
            </section>

            <section className="card p-5" aria-labelledby="cf-sessions">
              <h2 id="cf-sessions" className="mb-4 font-semibold">
                Сессии браузера
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
                        {s.busy ? 'выполняет запрос' : s.alive ? 'открыта' : 'закрыта'}
                        {s.lastUse ? ` · ${formatRelative(s.lastUse)}` : ''}
                        {s.consecutiveTimeouts ? ` · таймаутов подряд: ${s.consecutiveTimeouts}` : ''}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="text-sm text-muted">Сессий нет.</p>
              )}
              <p className="mt-4 text-xs text-muted">Каждый сайт за Cloudflare держит свою вкладку Chrome (300-600 МБ). Простаивающие сессии закрываются сами через {d.settings?.sessionIdleMinutes} мин.</p>
            </section>
          </div>

          <section className="card p-5" aria-labelledby="cf-settings">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
              <h2 id="cf-settings" className="font-semibold">
                Настройки и лимиты
              </h2>
              <div className="flex gap-2">
                <button type="button" className="btn btn-sm" onClick={resetStats} disabled={!!busy}>
                  <RotateCcw className="size-4" aria-hidden="true" /> Сбросить статистику
                </button>
                <Link to="/settings" className="btn btn-sm">
                  <Settings className="size-4" aria-hidden="true" /> Изменить настройки
                </Link>
              </div>
            </div>
            <dl className="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
              {[
                ['Адрес', d.settings?.url],
                ['Второй экземпляр (crawlUrl)', d.settings?.crawlUrl || 'нет'],
                ['Таймаут решения', `${Math.round((d.settings?.maxTimeoutMs || 0) / 1000)} с`],
                ['Закрывать сессию после простоя', `${d.settings?.sessionIdleMinutes} мин`],
                ['Пересоздавать после таймаутов подряд', d.settings?.recycleAfterTimeouts],
                ['Хост считается защищённым', `${d.settings?.guardedHours} ч`],
                ['cffetch', d.cffetch?.enabled ? `${d.cffetch.url} · ${d.cffetch.impersonate}` : 'выключен'],
                ['Защищённые хосты', (d.guarded || []).map((g) => g.host).join(', ') || 'нет'],
              ].map(([k, v]) => (
                <div key={k} className="flex justify-between gap-3 border-b border-border py-1.5">
                  <dt className="text-muted">{k}</dt>
                  <dd className="text-right font-medium break-all">{String(v ?? '-')}</dd>
                </div>
              ))}
            </dl>
            <p className="mt-4 text-xs text-muted">
              Лимиты процессора и памяти задаются контейнеру FlareSolverr, а не CrabIndex. Поменять их на ходу, без перезапуска:
            </p>
            <code className="mt-2 block overflow-x-auto rounded-lg bg-bg px-3 py-2 font-mono text-xs">docker update --cpus 2 --memory 4g --memory-swap 4g flaresolverr</code>
          </section>
        </>
      ) : null}
    </>
  )
}
