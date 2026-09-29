// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useId, useState } from 'react'
import { ClipboardCheck, CloudLightning, Database, HardDriveDownload, Play, Search, ShieldAlert, Stethoscope, Wand2 } from 'lucide-react'
import { getDataCheck, runPath, startFixAll } from '../lib/api.js'
import { CHECK_MODES, DIAGNOSTICS, MIGRATIONS } from '../lib/maintenance.js'
import { cleanParams } from '../lib/actions.js'
import { usePolling } from '../hooks/usePolling.js'
import { useResult } from '../components/ResultDrawer.jsx'
import { useConfirm } from '../components/Confirm.jsx'
import { Modal } from '../components/Modal.jsx'
import { PageHeader, Spinner, StatusDot } from '../components/ui.jsx'
import { formatDate, formatNumber, formatRelative } from '../lib/format.js'
import { useT } from '../lang/index.jsx'

function Section({ icon: Icon, title, description, children, id }) {
  return (
    <section className="card p-5" aria-labelledby={id}>
      <div className="mb-4 flex items-start gap-3">
        <Icon className="mt-0.5 size-5 shrink-0 text-accent" aria-hidden="true" />
        <div>
          <h2 id={id} className="font-semibold">
            {title}
          </h2>
          {description ? <p className="mt-0.5 text-sm text-muted">{description}</p> : null}
        </div>
      </div>
      {children}
    </section>
  )
}

function ParamsModal({ item, onClose, onSubmit }) {
  const t = useT()
  const [values, setValues] = useState({})
  const formId = useId()
  const missing = (item.params || []).some((p) => p.required && !String(values[p.name] ?? '').trim())
  return (
    <Modal
      open
      onClose={onClose}
      title={item.label}
      description={item.description}
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="submit" form={formId} className={`btn ${item.destructive ? 'btn-danger' : 'btn-primary'}`} disabled={missing}>
            <Play className="size-4" aria-hidden="true" /> {t('execute')}
          </button>
        </>
      }
    >
      <form
        id={formId}
        className="space-y-3"
        onSubmit={(e) => {
          e.preventDefault()
          if (!missing) onSubmit(cleanParams(values))
        }}
      >
        {item.params.map((p, i) => (
          <div key={p.name}>
            <label className="label" htmlFor={`${formId}-${p.name}`}>
              {p.label} <span className="font-mono">({p.name})</span>
              {p.required ? ' *' : ''}
            </label>
            <input
              id={`${formId}-${p.name}`}
              className="input"
              type={p.type === 'number' ? 'number' : 'text'}
              placeholder={p.placeholder}
              required={p.required}
              data-autofocus={i === 0 ? true : undefined}
              value={values[p.name] ?? ''}
              onChange={(e) => setValues((v) => ({ ...v, [p.name]: e.target.value }))}
            />
          </div>
        ))}
        {item.destructive ? <p className="text-sm text-danger">{t('mt_backup_note')}</p> : null}
      </form>
    </Modal>
  )
}

const DATA_COLUMNS = [
  ['zeroSize', 'mt_data_zero_size'],
  ['dupIds', 'mt_data_dup_ids'],
  ['foreignHost', 'mt_data_foreign_host'],
  ['badNames', 'mt_data_bad_names'],
]

/** Last read-only data-quality report (`/dev/checkdata`, weekly cron) with fix buttons. */
/// "Fix all": every migration the last report asks for, one after another on the server
/// (`dev/fixall`), then the check runs again. Progress comes with the check status.
function FixAllRow({ fixAll, running, reload }) {
  const t = useT()
  const confirm = useConfirm()
  const [error, setError] = useState(null)
  const plan = fixAll?.plan || []
  const steps = fixAll?.steps || []
  const labelOf = (path) => MIGRATIONS.find((m) => m.path === path)?.label || path
  const start = async () => {
    const ok = await confirm({
      title: t('mt_data_fix_all'),
      message: (
        <>
          <p>{t('mt_data_fix_all_confirm', { n: plan.length })}</p>
          <ol className="list-decimal pl-5">
            {plan.map((p) => (
              <li key={p}>
                {labelOf(p)} <span className="font-mono text-xs text-muted">{p}</span>
              </li>
            ))}
          </ol>
          <p className="text-muted">{t('mt_data_fix_all_note')}</p>
        </>
      ),
      danger: true,
      confirmLabel: t('execute'),
    })
    if (!ok) return
    setError(null)
    try {
      const r = await startFixAll()
      if (r && r.ok === false) setError(r.error || 'error')
    } catch (e) {
      setError(String(e?.message || e))
    }
    setTimeout(reload, 500)
  }
  if (fixAll?.running) {
    const idx = steps.findIndex((s) => s.status === 'running')
    const done = steps.filter((s) => s.status === 'done' || s.status === 'failed').length
    return (
      <p className="flex flex-wrap items-center gap-2 text-sm text-muted">
        <Spinner />
        {fixAll.phase === 'checking'
          ? t('mt_data_fix_all_checking', { n: steps.length })
          : t('mt_data_fix_all_progress', { n: done + 1, total: steps.length, step: idx >= 0 ? labelOf(steps[idx].path) : '…' })}
      </p>
    )
  }
  const finished = fixAll?.phase === 'done' && steps.length
  return (
    <div className="flex flex-wrap items-center gap-3 text-sm">
      {plan.length ? (
        <button type="button" className="btn" onClick={start} disabled={running}>
          <Wand2 className="size-4" aria-hidden="true" />
          {t('mt_data_fix_all')} <span className="text-xs opacity-70">({plan.length})</span>
        </button>
      ) : null}
      {finished ? (
        <span className="text-muted" title={steps.map((s) => `${s.path}: ${s.status}${s.result ? ' ' + JSON.stringify(s.result) : ''}`).join('\n')}>
          {steps.some((s) => s.status === 'failed')
            ? t('mt_data_fix_all_failed', { failed: steps.filter((s) => s.status === 'failed').map((s) => labelOf(s.path)).join(', ') })
            : t('mt_data_fix_all_done', { n: steps.length, when: formatRelative(fixAll.finishedAt) })}
        </span>
      ) : null}
      {error ? <span className="text-danger">{error}</span> : null}
    </div>
  )
}

function DataCheckSection({ busy, onRun, onFix }) {
  const t = useT()
  const [fast, setFast] = useState(false)
  const { data, reload } = usePolling(
    () =>
      getDataCheck().then((r) => {
        setFast(!!r.fixAll?.running)
        return { last: r.last || null, running: !!r.running, fixAll: r.fixAll || null }
      }),
    fast ? 3_000 : 15_000,
  )
  const last = data?.last && typeof data.last === 'object' ? data.last : null
  const running = !!data?.running
  const fixes = last?.fixes || {}
  const total = last?.total || {}
  const fixFor = (col, tracker) => (col === 'foreignHost' ? fixes.foreignHost?.[tracker] : fixes[col])
  // previous run (kept by the server when a migration re-checks): "was N" next to each cell
  const prevByTracker = new Map((last?.previous?.trackers || []).map((r) => [r.tracker, r]))
  return (
    <Section id="mt-data" icon={ClipboardCheck} title={t('mt_data_title')} description={t('mt_data_desc')}>
      <div className="mb-3 flex flex-wrap items-center gap-3 text-sm">
        <button type="button" className="btn" onClick={() => { onRun(); setTimeout(reload, 2000) }} disabled={busy}>
          {busy ? <Spinner /> : <Play className="size-4" aria-hidden="true" />}
          {t('mt_data_run')}
        </button>
        {running ? <span className="text-muted">{t('mt_data_running')}</span> : null}
        {last ? (
          <span className="text-muted" title={formatDate(last.at)}>
            {t('mt_data_last', { when: formatRelative(last.at), took: formatNumber(last.tookSec || 0) })}
            {last.trigger && last.trigger !== 'manual' && last.trigger !== 'cron' ? ` · ${t('mt_data_after_fix', { trigger: last.trigger })}` : ''}
          </span>
        ) : (
          <span className="text-muted">{t('mt_data_never')}</span>
        )}
      </div>
      {last ? (
        total.issues > 0 ? (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">{t('tr_col_tracker')}</th>
                  <th scope="col" className="text-right">{t('mt_data_rows')}</th>
                  {DATA_COLUMNS.map(([col, key]) => (
                    <th key={col} scope="col" className="text-right">{t(key)}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {(last.trackers || []).filter((r) => r.issues > 0 || (prevByTracker.get(r.tracker)?.issues || 0) > 0).map((r) => (
                  <tr key={r.tracker}>
                    <td className="font-mono text-xs">{r.tracker}</td>
                    <td className="text-right tabular-nums">{formatNumber(r.rows)}</td>
                    {DATA_COLUMNS.map(([col]) => {
                      const fix = r[col] > 0 ? fixFor(col, r.tracker) : null
                      const before = prevByTracker.get(r.tracker)
                      const changed = before && Number(before[col] || 0) !== Number(r[col] || 0)
                      return (
                        <td key={col} className="text-right tabular-nums">
                          {r[col] > 0 ? (
                            fix ? (
                              <button type="button" className="underline decoration-dotted underline-offset-2 hover:text-accent" title={fix} onClick={() => onFix(fix)}>
                                {formatNumber(r[col])}
                              </button>
                            ) : (
                              formatNumber(r[col])
                            )
                          ) : (
                            <span className="text-muted">0</span>
                          )}
                          {changed ? <span className="ml-1 text-xs text-muted">({t('mt_data_was', { n: formatNumber(before[col] || 0) })})</span> : null}
                        </td>
                      )
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-ok">
            {t('mt_data_clean', { rows: formatNumber(total.rows || 0) })}
            {last.previous && Number(last.previous.total?.issues || 0) > 0 ? <span className="ml-1 text-muted">({t('mt_data_was', { n: formatNumber(last.previous.total.issues) })})</span> : null}
          </p>
        )
      ) : null}
      {last ? <div className="mt-3"><FixAllRow fixAll={data?.fixAll} running={running || busy} reload={reload} /></div> : null}
      <p className="mt-3 text-xs text-muted">{t('mt_data_hint')}</p>
    </Section>
  )
}

export function MaintenancePage() {
  const t = useT()
  const { run } = useResult()
  const confirm = useConfirm()
  const [busy, setBusy] = useState(null)
  const [mode, setMode] = useState('report')
  const [paramsFor, setParamsFor] = useState(null)
  const status = usePolling(() => runPath('cron/maintenance/status').then((r) => r.data), 5000)
  const checkRunning = !!(status.data && typeof status.data === 'object' && status.data.running)

  const exec = async (key, title, path, query) => {
    setBusy(key)
    await run(title, () => runPath(path, query))
    setBusy(null)
  }

  const runCheck = async () => {
    const m = CHECK_MODES.find((x) => x.id === mode)
    if (m.destructive) {
      const ok = await confirm({
        title: t('mt_check_confirm_title', { mode: m.label }),
        message: (
          <>
            <p>{m.description}</p>
            <p className="text-muted">{t('mt_check_mode_warns', { id: m.id })}</p>
          </>
        ),
        danger: true,
        confirmLabel: t('run'),
      })
      if (!ok) return
    }
    await exec('check', t('mt_check_exec_title', { id: m.id }), 'cron/maintenance/check', { mode })
    status.reload()
  }

  const runMigration = async (item) => {
    if (item.params?.length) {
      setParamsFor({ ...item, destructive: true })
      return
    }
    const ok = await confirm({
      title: item.label,
      message: (
        <>
          <p>{item.description}</p>
          <p className="text-muted">{t('mt_mig_confirm_body', { path: item.path })}</p>
        </>
      ),
      danger: true,
      confirmLabel: t('execute'),
    })
    if (ok) exec(item.path, item.label, item.path)
  }

  const onParams = async (query) => {
    const item = paramsFor
    setParamsFor(null)
    if (item.destructive) {
      const ok = await confirm({
        title: item.label,
        message: <p>{t('mt_params_confirm', { path: item.path, query: new URLSearchParams(query).toString() || '-' })}</p>,
        danger: true,
        confirmLabel: t('execute'),
      })
      if (!ok) return
    }
    exec(item.path, item.label, item.path, query)
  }

  return (
    <>
      <PageHeader title={t('nav_maintenance')} description={t('mt_desc')} />
      <div className="grid gap-6 xl:grid-cols-2">
        <Section id="mt-check" icon={Stethoscope} title={t('mt_check_title')} description="cron/maintenance/check">
          <fieldset className="space-y-2">
            <legend className="sr-only">{t('mt_check_mode')}</legend>
            {CHECK_MODES.map((m) => (
              <label key={m.id} className={`flex cursor-pointer gap-3 rounded-lg border p-3 ${mode === m.id ? 'border-brand bg-brand/5' : 'border-border'}`}>
                <input type="radio" name="check-mode" value={m.id} checked={mode === m.id} onChange={() => setMode(m.id)} className="mt-1 accent-[#e85d24]" />
                <span>
                  <span className="text-sm font-medium">
                    {m.label} <span className="font-mono text-xs text-muted">mode={m.id}</span>
                  </span>
                  <span className="block text-xs text-muted">{m.description}</span>
                </span>
              </label>
            ))}
          </fieldset>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            <button type="button" className={`btn ${mode === 'report' ? 'btn-primary' : 'btn-danger'}`} onClick={runCheck} disabled={busy === 'check' || checkRunning}>
              {busy === 'check' ? <Spinner /> : <Play className="size-4" aria-hidden="true" />}
              {t('mt_check_run')}
            </button>
            <button type="button" className="btn" onClick={() => exec('status', t('mt_check_title_status'), 'cron/maintenance/status')}>
              {t('status')}
            </button>
            <span className="text-sm text-muted" aria-live="polite">
              <StatusDot tone={checkRunning ? 'brand' : 'muted'} label={checkRunning ? t('mt_check_running') : t('mt_check_idle')} />
            </span>
          </div>
        </Section>

        <div className="space-y-6">
          <Section id="mt-db" icon={Database} title={t('mt_db_title')} description={t('mt_db_desc')}>
            <button type="button" className="btn" onClick={() => exec('jsondb', t('mt_db_save_title'), 'jsondb/save')} disabled={busy === 'jsondb'}>
              {busy === 'jsondb' ? <Spinner /> : <HardDriveDownload className="size-4" aria-hidden="true" />}
              {t('mt_db_save_btn')}
            </button>
          </Section>
          <Section id="mt-cf" icon={CloudLightning} title={t('mt_cf_title')} description={t('mt_cf_desc')}>
            <button type="button" className="btn" onClick={() => exec('cf', t('mt_cf_warmup_title'), 'cron/cloudflare/warmup')} disabled={busy === 'cf'}>
              {busy === 'cf' ? <Spinner /> : <CloudLightning className="size-4" aria-hidden="true" />}
              {t('mt_cf_warmup_btn')}
            </button>
          </Section>
        </div>

        <DataCheckSection busy={busy === 'dev/checkdata'} onRun={() => exec('dev/checkdata', t('mt_data_title'), 'dev/checkdata')} onFix={(path) => runMigration(MIGRATIONS.find((m) => m.path === path) || { path, label: path, description: '' })} />

        <Section id="mt-diag" icon={Search} title={t('mt_diag_title')} description={t('mt_diag_desc')}>
          <ul className="divide-y divide-border">
            {DIAGNOSTICS.map((d) => (
              <li key={d.path} className="flex flex-wrap items-center justify-between gap-3 py-3 first:pt-0 last:pb-0">
                <div className="min-w-0">
                  <p className="text-sm font-medium">{d.label}</p>
                  <p className="text-xs text-muted">{d.description}</p>
                </div>
                <button type="button" className="btn btn-sm" onClick={() => setParamsFor(d)} disabled={busy === d.path}>
                  {busy === d.path ? <Spinner className="size-3.5" /> : null}
                  {t('run')}
                </button>
              </li>
            ))}
          </ul>
        </Section>

        <Section id="mt-mig" icon={ShieldAlert} title={t('mt_mig_title')} description={t('mt_mig_desc')}>
          <ul className="divide-y divide-border">
            {MIGRATIONS.map((m) => (
              <li key={m.path} className="flex flex-wrap items-center justify-between gap-3 py-3 first:pt-0 last:pb-0">
                <div className="min-w-0">
                  <p className="text-sm font-medium">{m.label}</p>
                  <p className="text-xs text-muted">
                    {m.description} <span className="font-mono">{m.path}</span>
                  </p>
                </div>
                <button type="button" className="btn btn-sm border-danger/40 text-danger" onClick={() => runMigration(m)} disabled={busy === m.path}>
                  {busy === m.path ? <Spinner className="size-3.5" /> : null}
                  {t('execute')}
                </button>
              </li>
            ))}
          </ul>
        </Section>
      </div>
      {paramsFor ? <ParamsModal item={paramsFor} onClose={() => setParamsFor(null)} onSubmit={onParams} /> : null}
    </>
  )
}
