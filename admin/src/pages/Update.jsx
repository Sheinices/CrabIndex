import { useEffect, useRef, useState } from 'react'
import { Download, ExternalLink, RefreshCw } from 'lucide-react'
import { applyUpdate, getSession, getUpdate } from '../lib/api.js'
import { isBusy, notesLines, reachedTarget, stageLabel } from '../lib/update.js'
import { useConfirm } from '../components/Confirm.jsx'
import { useToast } from '../components/Toast.jsx'
import { ErrorBox, PageHeader, Spinner, StatusDot } from '../components/ui.jsx'
import { formatDate } from '../lib/format.js'

const POLL_MS = 2000
const RESTART_TIMEOUT_MS = 3 * 60 * 1000

export function UpdatePage() {
  const [info, setInfo] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const [checking, setChecking] = useState(false)
  // null | { target, stage, message, restarting }
  const [progress, setProgress] = useState(null)
  const confirm = useConfirm()
  const toast = useToast()
  const timer = useRef(null)

  const load = async (force = false) => {
    setChecking(force)
    try {
      const r = await getUpdate(force)
      setInfo(r)
      setError(null)
      if (isBusy(r.state)) setProgress((p) => p || { target: r.state.target, stage: r.state.stage, message: r.state.message })
      return r
    } catch (e) {
      setError(e)
      return null
    } finally {
      setLoading(false)
      setChecking(false)
    }
  }

  useEffect(() => {
    load(false)
    return () => clearTimeout(timer.current)
  }, [])

  // While an update runs: poll the state; once the server goes down, wait for the new version.
  useEffect(() => {
    if (!progress) return undefined
    const started = Date.now()
    let stopped = false
    const tick = async () => {
      if (stopped) return
      if (Date.now() - started > RESTART_TIMEOUT_MS) {
        setProgress((p) => p && { ...p, stage: 'error', message: 'Служба не вернулась за 3 минуты. Проверьте на сервере: systemctl status crabindex' })
        return
      }
      if (!progress.restarting) {
        try {
          const r = await getUpdate(false, { silent401: true })
          const st = r.state || {}
          if (st.stage === 'error') {
            setProgress(null)
            setInfo(r)
            toast.error('Обновление не выполнено', st.message)
            return
          }
          setProgress((p) => p && { ...p, stage: st.stage || p.stage, message: st.message || p.message, restarting: st.stage === 'restarting' })
        } catch {
          setProgress((p) => p && { ...p, stage: 'restarting', message: 'Служба перезапускается…', restarting: true })
        }
      } else {
        try {
          const s = await getSession()
          if (reachedTarget(s?.version, progress.target)) {
            toast.success(`CrabIndex обновлён до ${progress.target}`)
            timer.current = setTimeout(() => window.location.reload(), 1200)
            return
          }
        } catch {
          /* still restarting */
        }
      }
      timer.current = setTimeout(tick, POLL_MS)
    }
    timer.current = setTimeout(tick, POLL_MS)
    return () => {
      stopped = true
      clearTimeout(timer.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [progress?.restarting, progress?.target])

  const start = async () => {
    const target = info?.latest?.version
    const ok = await confirm({
      title: `Обновить CrabIndex до ${target}?`,
      message: (
        <>
          <p>Архив будет скачан с GitHub и проверен по SHA256SUMS релиза. Затем CrabIndex сохранит базу, остановится и systemd запустит новую версию.</p>
          <p>Служба будет недоступна около 10-30 секунд. Конфиг и база не меняются.</p>
        </>
      ),
      confirmLabel: 'Обновить',
    })
    if (!ok) return
    try {
      const r = await applyUpdate()
      setProgress({ target: r.target, stage: 'downloading', message: 'Загрузка архива', restarting: false })
    } catch (e) {
      if (e?.status !== 401) toast.error('Обновление не запущено', e?.message || String(e))
    }
  }

  const latest = info?.latest
  const notes = notesLines(latest?.notes)

  return (
    <>
      <PageHeader
        title="Обновление"
        description="Версия CrabIndex и обновление до нового релиза с GitHub"
        actions={
          <button type="button" className="btn btn-sm" onClick={() => load(true)} disabled={checking || !!progress}>
            {checking ? <Spinner /> : <RefreshCw className="size-4" aria-hidden="true" />} Проверить сейчас
          </button>
        }
      />
      <ErrorBox error={error} onRetry={() => load(true)} />
      {loading ? (
        <Spinner label="Проверка версии…" />
      ) : info ? (
        <div className="grid gap-6 lg:grid-cols-3">
          <section className="card p-5 lg:col-span-1" aria-labelledby="upd-status">
            <h2 id="upd-status" className="mb-4 font-semibold">
              Версии
            </h2>
            <dl className="space-y-3 text-sm">
              <div className="flex justify-between gap-3">
                <dt className="text-muted">Установлена</dt>
                <dd className="font-semibold tabular-nums">{info.current}</dd>
              </div>
              <div className="flex justify-between gap-3">
                <dt className="text-muted">Последняя</dt>
                <dd className="font-semibold tabular-nums">{latest ? latest.version : '-'}</dd>
              </div>
              {latest?.publishedAt ? (
                <div className="flex justify-between gap-3">
                  <dt className="text-muted">Вышла</dt>
                  <dd>{formatDate(latest.publishedAt)}</dd>
                </div>
              ) : null}
            </dl>
            <div className="mt-5">
              {info.error ? (
                <p className="text-sm text-danger">Не удалось проверить: {info.error}</p>
              ) : info.available ? (
                <StatusDot tone="warn" label={`Доступна версия ${latest.version}`} />
              ) : (
                <StatusDot tone="ok" label="Установлена актуальная версия" />
              )}
            </div>

            {progress ? (
              <div className="mt-5 rounded-xl border border-border bg-surface-2 p-4 text-sm" role="status" aria-live="polite">
                <div className="flex items-center gap-2 font-medium">
                  {progress.stage === 'error' ? null : <Spinner />}
                  {stageLabel(progress.stage)}
                </div>
                <p className="mt-1 text-muted">{progress.message}</p>
              </div>
            ) : info.available ? (
              info.canSelfUpdate ? (
                <button type="button" className="btn btn-primary mt-5 w-full" onClick={start}>
                  <Download className="size-4" aria-hidden="true" /> Обновить до {latest.version}
                </button>
              ) : (
                <div className="mt-5 rounded-xl border border-border bg-surface-2 p-4 text-sm">
                  <p className="font-medium">Обновление из панели недоступно</p>
                  <p className="mt-1 text-muted">{info.reason}</p>
                </div>
              )
            ) : null}
            {info.state?.stage === 'error' && !progress ? <p className="mt-4 text-sm text-danger">Последняя попытка: {info.state.message}</p> : null}
          </section>

          <section className="card p-5 lg:col-span-2" aria-labelledby="upd-notes">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
              <h2 id="upd-notes" className="font-semibold">
                {latest ? `Что нового в ${latest.name || latest.version}` : 'Что нового'}
              </h2>
              {latest?.url ? (
                <a href={latest.url} target="_blank" rel="noreferrer" className="btn btn-sm btn-ghost">
                  <ExternalLink className="size-4" aria-hidden="true" /> Релиз на GitHub
                </a>
              ) : null}
            </div>
            {notes.length ? (
              <div className="space-y-1 text-sm whitespace-pre-wrap">
                {notes.map((l, i) => (
                  <p key={i}>{l || ' '}</p>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted">Описание релиза пустое.</p>
            )}
            <p className="mt-6 text-xs text-muted">
              Проверка обращается к GitHub, только когда открыта эта страница, и запоминает ответ на 6 часов. В Docker обновляйте образ (docker compose pull), при ручной установке - установщиком: sudo bash install.sh --update.
            </p>
          </section>
        </div>
      ) : null}
    </>
  )
}
