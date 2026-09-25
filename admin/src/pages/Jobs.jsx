import { useState } from 'react'
import { PlayCircle, RefreshCw } from 'lucide-react'
import { getBackgroundJobs, getParseAllStatus, resumeParseAll } from '../lib/api.js'
import { usePolling } from '../hooks/usePolling.js'
import { useResult } from '../components/ResultDrawer.jsx'
import { ErrorBox, PageHeader, ProgressBar, Spinner, StatusDot, Toggle } from '../components/ui.jsx'
import { JobList } from './Overview.jsx'
import { parseAllProgress } from './Trackers.jsx'
import { formatNumber } from '../lib/format.js'

export function JobsPage() {
  const [live, setLive] = useState(true)
  const jobs = usePolling(() => getBackgroundJobs(), 5000, { enabled: live })
  const pa = usePolling(() => getParseAllStatus(), 5000, { enabled: live })
  const { run } = useResult()
  const [busy, setBusy] = useState(false)

  const reload = () => {
    jobs.reload()
    pa.reload()
  }

  const resume = async () => {
    setBusy(true)
    await run('Возобновить ParseAll', () => resumeParseAll())
    setBusy(false)
    reload()
  }

  const list = Array.isArray(pa.data) ? pa.data : []

  return (
    <>
      <PageHeader
        title="Задачи"
        description="Фоновые задачи парсеров и состояние циклов ParseAll"
        actions={
          <>
            <Toggle id="jobs-live" checked={live} onChange={setLive} label="Автообновление" />
            <button type="button" className="btn btn-sm" onClick={reload}>
              <RefreshCw className="size-4" aria-hidden="true" /> Обновить
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={resume} disabled={busy}>
              {busy ? <Spinner /> : <PlayCircle className="size-4" aria-hidden="true" />}
              Возобновить ParseAll
            </button>
          </>
        }
      />
      <ErrorBox error={jobs.error || pa.error} onRetry={reload} />
      <div className="grid gap-6 lg:grid-cols-2">
        <section className="card p-5" aria-labelledby="jobs-active">
          <h2 id="jobs-active" className="mb-4 font-semibold">
            Активные задачи
          </h2>
          {jobs.loading && !jobs.data ? <Spinner label="Загрузка…" /> : <JobList jobs={jobs.data?.jobs} />}
        </section>
        <section className="card p-5" aria-labelledby="jobs-pa">
          <h2 id="jobs-pa" className="mb-4 font-semibold">
            ParseAll
          </h2>
          {pa.loading && !pa.data ? (
            <Spinner label="Загрузка…" />
          ) : list.length ? (
            <ul className="space-y-4">
              {list.map((p) => {
                const pct = parseAllProgress(p)
                return (
                  <li key={p.tracker}>
                    <div className="mb-1.5 flex items-baseline justify-between gap-2 text-sm">
                      <span className="flex items-center gap-2 font-medium">
                        <StatusDot tone={p.running ? 'brand' : p.pending > 0 ? 'warn' : 'muted'} />
                        {p.tracker}
                      </span>
                      <span className="text-xs text-muted tabular-nums">
                        {p.running ? 'идёт' : p.pending > 0 ? 'приостановлен' : 'нет цикла'} · {formatNumber(p.pending)} / {formatNumber(p.mapCount)}
                      </span>
                    </div>
                    <ProgressBar value={pct ?? 0} label={`ParseAll ${p.tracker}`} />
                  </li>
                )
              })}
            </ul>
          ) : (
            <p className="text-sm text-muted">Нет данных</p>
          )}
        </section>
      </div>
    </>
  )
}
