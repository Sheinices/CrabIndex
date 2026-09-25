import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Activity, Gauge, RefreshCw, ShieldBan, Users } from 'lucide-react'
import { getWafOverview } from '../../lib/api.js'
import { usePolling } from '../../hooks/usePolling.js'
import { ErrorBox, Spinner, Toggle } from '../../components/ui.jsx'
import { TimelineChart } from '../../components/TimelineChart.jsx'
import { formatDate, formatNumber, formatRelative } from '../../lib/format.js'
import { BLOCK_REASONS, percent } from '../../lib/waf.js'
import { DisabledNotice, Empty, SETTINGS_WAF } from './shared.jsx'
import { useWaf } from './Waf.jsx'

const SERIES = [
  { key: 'requests', label: 'Запросы', color: 'text-chart-1', area: true },
  { key: 'blocked', label: 'Заблокировано', color: 'text-chart-2' },
]

const STATUS_ROWS = [
  { key: '2xx', label: '2xx успешно', bar: 'bg-ok' },
  { key: '3xx', label: '3xx редиректы', bar: 'bg-muted' },
  { key: '4xx', label: '4xx ошибки клиента', bar: 'bg-warn' },
  { key: '5xx', label: '5xx ошибки сервера', bar: 'bg-danger' },
]

const BAR_TONE = { danger: 'bg-danger', warn: 'bg-warn' }

function Stat({ icon: Icon, label, value, hint }) {
  return (
    <div className="card p-4">
      <div className="flex items-center gap-2 text-xs font-medium text-muted">
        <Icon className="size-4" aria-hidden="true" />
        {label}
      </div>
      <p className="mt-2 text-2xl font-semibold tabular-nums">{value}</p>
      {hint ? <p className="mt-1 truncate text-xs text-muted">{hint}</p> : null}
    </div>
  )
}

export function Breakdown({ rows, total, empty = 'Нет данных' }) {
  if (!(total > 0)) return <p className="text-sm text-muted">{empty}</p>
  return (
    <ul className="space-y-3">
      {rows.map((r) => {
        const pct = percent(r.value, total)
        return (
          <li key={r.key}>
            <div className="mb-1 flex items-baseline justify-between gap-2 text-sm">
              <span>{r.label}</span>
              <span className="text-xs text-muted tabular-nums">
                <span className="text-fg">{formatNumber(r.value)}</span> · {pct}%
              </span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-surface-2" aria-hidden="true">
              <div className={`h-full rounded-full ${r.bar}`} style={{ width: `${pct}%` }} />
            </div>
          </li>
        )
      })}
    </ul>
  )
}

const windowTime = (win) => (t) => {
  const d = new Date(t)
  if (Number.isNaN(d.getTime())) return String(t ?? '')
  return d.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' }) + (win === '24h' && d.getHours() === 0 && d.getMinutes() < 10 ? ` ${d.getDate()}.${d.getMonth() + 1}` : '')
}

export function WafOverview() {
  const { enabled } = useWaf()
  const [win, setWin] = useState('60m')
  const [auto, setAuto] = useState(true)
  const { data, error, loading, reload } = usePolling(() => getWafOverview(win), auto ? 10_000 : 0, { enabled: auto })

  useEffect(() => {
    reload()
  }, [win, reload])

  const o = data || {}
  const totals = o.totals || {}
  const codes = o.statusCodes || {}
  const reasons = o.blockedByReason || {}
  const codeTotal = STATUS_ROWS.reduce((a, r) => a + (Number(codes[r.key]) || 0), 0)
  const reasonRows = Object.keys(BLOCK_REASONS)
    .filter((k) => k !== 'manual')
    .map((k) => ({ key: k, label: BLOCK_REASONS[k].label, value: Number(reasons[k]) || 0, bar: BAR_TONE[BLOCK_REASONS[k].tone] }))
  const reasonTotal = reasonRows.reduce((a, r) => a + r.value, 0)

  return (
    <div className="space-y-6">
      {data && data.enabled === false && enabled ? <DisabledNotice /> : null}
      {data?.logRequests === false ? (
        <p role="status" className="card border-warn/40 bg-warn/10 p-4 text-sm">
          Журнал запросов выключен (waf.logRequests) — статистика и журнал не пополняются.{' '}
          <Link to={SETTINGS_WAF} className="text-accent hover:underline">
            Настройки WAF
          </Link>
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-3">
        <div role="group" aria-label="Окно статистики" className="inline-flex rounded-lg border border-border p-0.5">
          {[
            ['60m', '60 мин'],
            ['24h', '24 ч'],
          ].map(([v, l]) => (
            <button
              key={v}
              type="button"
              aria-pressed={win === v}
              onClick={() => setWin(v)}
              className={`rounded-md px-3 py-1 text-sm ${win === v ? 'bg-brand/15 font-medium text-accent' : 'text-muted hover:text-fg'}`}
            >
              {l}
            </button>
          ))}
        </div>
        {o.since ? <span className="text-xs text-muted">Статистика с {formatDate(o.since)}</span> : null}
        <div className="ml-auto flex items-center gap-3">
          <Toggle id="waf-ov-auto" checked={auto} onChange={setAuto} label="Автообновление 10 с" />
          <button type="button" className="btn btn-sm" onClick={reload} aria-label="Обновить">
            <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
          </button>
        </div>
      </div>
      <ErrorBox error={error} onRetry={reload} />
      {loading && !data ? (
        <Spinner className="size-5" label="Загрузка…" />
      ) : (
        <>
          <section aria-label="Итоги" className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
            <Stat icon={Activity} label="Запросов" value={formatNumber(totals.requests)} hint={win === '60m' ? 'за 60 минут' : 'за 24 часа'} />
            <Stat
              icon={ShieldBan}
              label="Заблокировано"
              value={formatNumber(totals.blocked)}
              hint={`${percent(totals.blocked, totals.requests)}% запросов`}
            />
            <Stat icon={Users} label="Уникальных IP" value={formatNumber(totals.uniqueIps)} />
            <Stat
              icon={Gauge}
              label="Запросов/сек"
              value={Number.isFinite(Number(totals.rps)) ? Number(totals.rps).toLocaleString('ru-RU', { maximumFractionDigits: 2 }) : '-'}
              hint="в среднем за окно"
            />
          </section>

          <section className="card p-5" aria-labelledby="waf-tl">
            <h2 id="waf-tl" className="mb-3 font-semibold">
              Запросы и блокировки{' '}
              <span className="text-sm font-normal text-muted">{win === '60m' ? '· по минутам' : '· по 10 минут'}</span>
            </h2>
            <TimelineChart data={o.timeline} series={SERIES} formatX={windowTime(win)} label="Запросы и блокировки" />
          </section>

          <div className="grid gap-6 lg:grid-cols-2">
            <section className="card p-5" aria-labelledby="waf-codes">
              <h2 id="waf-codes" className="mb-4 font-semibold">
                Коды ответа
              </h2>
              <Breakdown rows={STATUS_ROWS.map((r) => ({ ...r, value: Number(codes[r.key]) || 0 }))} total={codeTotal} />
            </section>
            <section className="card p-5" aria-labelledby="waf-reasons">
              <h2 id="waf-reasons" className="mb-4 font-semibold">
                Причины блокировок
              </h2>
              <Breakdown rows={reasonRows} total={reasonTotal} empty="Блокировок не было" />
            </section>
          </div>

          <div className="grid gap-6 lg:grid-cols-2">
            <section aria-labelledby="waf-top-ips">
              <h2 id="waf-top-ips" className="mb-3 font-semibold">
                Топ IP
              </h2>
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>IP</th>
                      <th className="text-right">Запросов</th>
                      <th className="text-right">Блок.</th>
                      <th>Последний</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(o.topIps || []).map((r) => (
                      <tr key={r.ip}>
                        <td>
                          <Link to={`/waf/log?ip=${encodeURIComponent(r.ip)}`} className="font-mono text-xs text-accent hover:underline">
                            {r.ip}
                          </Link>
                        </td>
                        <td className="text-right tabular-nums">{formatNumber(r.requests)}</td>
                        <td className={`text-right tabular-nums ${r.blocked ? 'text-danger' : 'text-muted'}`}>{formatNumber(r.blocked)}</td>
                        <td className="text-xs whitespace-nowrap text-muted">{formatRelative(r.lastSeen)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {!o.topIps?.length ? <Empty>Нет данных</Empty> : null}
              </div>
            </section>
            <section aria-labelledby="waf-top-paths">
              <h2 id="waf-top-paths" className="mb-3 font-semibold">
                Топ путей
              </h2>
              <div className="table-wrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>Путь</th>
                      <th className="text-right">Запросов</th>
                      <th className="text-right">Ошибок</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(o.topPaths || []).map((r) => (
                      <tr key={r.path}>
                        <td className="max-w-xs">
                          <Link
                            to={`/waf/log?path=${encodeURIComponent(r.path)}`}
                            className="block truncate font-mono text-xs text-accent hover:underline"
                            title={r.path}
                          >
                            {r.path}
                          </Link>
                        </td>
                        <td className="text-right tabular-nums">{formatNumber(r.requests)}</td>
                        <td className={`text-right tabular-nums ${r.errors ? 'text-warn' : 'text-muted'}`}>{formatNumber(r.errors)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {!o.topPaths?.length ? <Empty>Нет данных</Empty> : null}
              </div>
            </section>
          </div>
        </>
      )}
    </div>
  )
}
