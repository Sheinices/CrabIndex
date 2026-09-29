// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useEffect, useRef, useState } from 'react'
import { Download, ExternalLink, RefreshCw } from 'lucide-react'
import { applyUpdate, getSession, getUpdate } from '../lib/api.js'
import { isBusy, notesLines, reachedTarget, stageKey } from '../lib/update.js'
import { useConfirm } from '../components/Confirm.jsx'
import { useToast } from '../components/Toast.jsx'
import { ErrorBox, PageHeader, Spinner, StatusDot } from '../components/ui.jsx'
import { formatDate } from '../lib/format.js'
import { useT } from '../lang/index.jsx'

const POLL_MS = 2000
const RESTART_TIMEOUT_MS = 3 * 60 * 1000

export function UpdatePage() {
  const t = useT()
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
        setProgress((p) => p && { ...p, stage: 'error', message: t('update_not_returned') })
        return
      }
      if (!progress.restarting) {
        try {
          const r = await getUpdate(false, { silent401: true })
          const st = r.state || {}
          if (st.stage === 'error') {
            setProgress(null)
            setInfo(r)
            toast.error(t('update_toast_failed'), st.message)
            return
          }
          setProgress((p) => p && { ...p, stage: st.stage || p.stage, message: st.message || p.message, restarting: st.stage === 'restarting' })
        } catch {
          setProgress((p) => p && { ...p, stage: 'restarting', message: t('update_service_restarting'), restarting: true })
        }
      } else {
        try {
          const s = await getSession()
          if (reachedTarget(s?.version, progress.target)) {
            toast.success(t('update_toast_done', { version: progress.target }))
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
      title: t('update_confirm_title', { version: target }),
      message: (
        <>
          <p>{t('update_confirm_body_1')}</p>
          <p>{t('update_confirm_body_2')}</p>
        </>
      ),
      confirmLabel: t('update_confirm_ok'),
    })
    if (!ok) return
    try {
      const r = await applyUpdate()
      setProgress({ target: r.target, stage: 'downloading', message: t('update_started_download'), restarting: false })
    } catch (e) {
      if (e?.status !== 401) toast.error(t('update_toast_not_started'), e?.message || String(e))
    }
  }

  const latest = info?.latest
  const notes = notesLines(latest?.notes)

  return (
    <>
      <PageHeader
        title={t('nav_update')}
        description={t('update_desc')}
        actions={
          <button type="button" className="btn btn-sm" onClick={() => load(true)} disabled={checking || !!progress}>
            {checking ? <Spinner /> : <RefreshCw className="size-4" aria-hidden="true" />} {t('update_check_now')}
          </button>
        }
      />
      <ErrorBox error={error} onRetry={() => load(true)} />
      {loading ? (
        <Spinner label={t('update_checking')} />
      ) : info ? (
        <div className="grid gap-6 lg:grid-cols-3">
          <section className="card p-5 lg:col-span-1" aria-labelledby="upd-status">
            <h2 id="upd-status" className="mb-4 font-semibold">
              {t('update_versions')}
            </h2>
            <dl className="space-y-3 text-sm">
              <div className="flex justify-between gap-3">
                <dt className="text-muted">{t('update_installed')}</dt>
                <dd className="font-semibold tabular-nums">{info.current}</dd>
              </div>
              <div className="flex justify-between gap-3">
                <dt className="text-muted">{t('update_latest')}</dt>
                <dd className="font-semibold tabular-nums">{latest ? latest.version : '-'}</dd>
              </div>
              {latest?.publishedAt ? (
                <div className="flex justify-between gap-3">
                  <dt className="text-muted">{t('update_released')}</dt>
                  <dd>{formatDate(latest.publishedAt)}</dd>
                </div>
              ) : null}
            </dl>
            <div className="mt-5">
              {info.error ? (
                <p className="text-sm text-danger">{t('update_check_failed', { msg: info.error })}</p>
              ) : info.available ? (
                <StatusDot tone="warn" label={t('update_available_version', { version: latest.version })} />
              ) : (
                <StatusDot tone="ok" label={t('update_up_to_date')} />
              )}
            </div>

            {progress ? (
              <div className="mt-5 rounded-xl border border-border bg-surface-2 p-4 text-sm" role="status" aria-live="polite">
                <div className="flex items-center gap-2 font-medium">
                  {progress.stage === 'error' ? null : <Spinner />}
                  {t(stageKey(progress.stage))}
                </div>
                <p className="mt-1 text-muted">{progress.message}</p>
              </div>
            ) : info.available ? (
              info.canSelfUpdate ? (
                <button type="button" className="btn btn-primary mt-5 w-full" onClick={start}>
                  <Download className="size-4" aria-hidden="true" /> {t('update_to_version', { version: latest.version })}
                </button>
              ) : (
                <div className="mt-5 rounded-xl border border-border bg-surface-2 p-4 text-sm">
                  <p className="font-medium">{t('update_panel_unavailable')}</p>
                  <p className="mt-1 text-muted">{info.reason}</p>
                </div>
              )
            ) : null}
            {info.state?.stage === 'error' && !progress ? (
              <p className="mt-4 text-sm text-danger">{t('update_last_attempt', { msg: info.state.message })}</p>
            ) : null}
          </section>

          <section className="card p-5 lg:col-span-2" aria-labelledby="upd-notes">
            <div className="mb-4 flex flex-wrap items-center justify-between gap-2">
              <h2 id="upd-notes" className="font-semibold">
                {latest ? t('update_whats_new_in', { name: latest.name || latest.version }) : t('update_whats_new')}
              </h2>
              {latest?.url ? (
                <a href={latest.url} target="_blank" rel="noreferrer" className="btn btn-sm btn-ghost">
                  <ExternalLink className="size-4" aria-hidden="true" /> {t('update_release_on_github')}
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
              <p className="text-sm text-muted">{t('update_notes_empty')}</p>
            )}
            <p className="mt-6 text-xs text-muted">{t('update_github_note')}</p>
          </section>
        </div>
      ) : null}
    </>
  )
}
