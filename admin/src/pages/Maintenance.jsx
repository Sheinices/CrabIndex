import { useId, useState } from 'react'
import { CloudLightning, Database, HardDriveDownload, Play, Search, ShieldAlert, Stethoscope } from 'lucide-react'
import { runPath } from '../lib/api.js'
import { CHECK_MODES, DIAGNOSTICS, MIGRATIONS } from '../lib/maintenance.js'
import { cleanParams } from '../lib/actions.js'
import { usePolling } from '../hooks/usePolling.js'
import { useResult } from '../components/ResultDrawer.jsx'
import { useConfirm } from '../components/Confirm.jsx'
import { Modal } from '../components/Modal.jsx'
import { PageHeader, Spinner, StatusDot } from '../components/ui.jsx'

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
            Отмена
          </button>
          <button type="submit" form={formId} className={`btn ${item.destructive ? 'btn-danger' : 'btn-primary'}`} disabled={missing}>
            <Play className="size-4" aria-hidden="true" /> Выполнить
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
        {item.destructive ? <p className="text-sm text-danger">Операция изменяет базу данных. Сделайте резервную копию Data/fdb.</p> : null}
      </form>
    </Modal>
  )
}

export function MaintenancePage() {
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
        title: `Проверка FileDB: ${m.label}`,
        message: (
          <>
            <p>{m.description}</p>
            <p className="text-muted">Режим {m.id} изменяет базу. Рекомендуется резервная копия Data/fdb.</p>
          </>
        ),
        danger: true,
        confirmLabel: 'Запустить',
      })
      if (!ok) return
    }
    await exec('check', `Проверка FileDB (${m.id})`, 'cron/maintenance/check', { mode })
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
          <p className="text-muted">
            Операция проходит по всей базе и изменяет записи (<code className="font-mono">{item.path}</code>). Отменить её нельзя.
          </p>
        </>
      ),
      danger: true,
      confirmLabel: 'Выполнить',
    })
    if (ok) exec(item.path, item.label, item.path)
  }

  const onParams = async (query) => {
    const item = paramsFor
    setParamsFor(null)
    if (item.destructive) {
      const ok = await confirm({
        title: item.label,
        message: (
          <p>
            Выполнить <code className="font-mono">{item.path}</code> с параметрами{' '}
            <code className="font-mono">{new URLSearchParams(query).toString() || '-'}</code>?
          </p>
        ),
        danger: true,
        confirmLabel: 'Выполнить',
      })
      if (!ok) return
    }
    exec(item.path, item.label, item.path, query)
  }

  return (
    <>
      <PageHeader title="Обслуживание" description="Проверка базы, диагностика и миграции данных" />
      <div className="grid gap-6 xl:grid-cols-2">
        <Section id="mt-check" icon={Stethoscope} title="Проверка FileDB" description="cron/maintenance/check">
          <fieldset className="space-y-2">
            <legend className="sr-only">Режим проверки</legend>
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
              Запустить проверку
            </button>
            <button type="button" className="btn" onClick={() => exec('status', 'Статус проверки', 'cron/maintenance/status')}>
              Статус
            </button>
            <span className="text-sm text-muted" aria-live="polite">
              <StatusDot tone={checkRunning ? 'brand' : 'muted'} label={checkRunning ? 'проверка выполняется' : 'не выполняется'} />
            </span>
          </div>
        </Section>

        <div className="space-y-6">
          <Section id="mt-db" icon={Database} title="База данных" description="Сброс изменений на диск">
            <button type="button" className="btn" onClick={() => exec('jsondb', 'Сохранение БД', 'jsondb/save')} disabled={busy === 'jsondb'}>
              {busy === 'jsondb' ? <Spinner /> : <HardDriveDownload className="size-4" aria-hidden="true" />}
              Сохранить БД (jsondb/save)
            </button>
          </Section>
          <Section id="mt-cf" icon={CloudLightning} title="Cloudflare" description="Прогрев сессии FlareSolverr">
            <button type="button" className="btn" onClick={() => exec('cf', 'Прогрев Cloudflare', 'cron/cloudflare/warmup')} disabled={busy === 'cf'}>
              {busy === 'cf' ? <Spinner /> : <CloudLightning className="size-4" aria-hidden="true" />}
              Прогреть (warmup)
            </button>
          </Section>
        </div>

        <Section id="mt-diag" icon={Search} title="Диагностика" description="Только чтение, базу не изменяет">
          <ul className="divide-y divide-border">
            {DIAGNOSTICS.map((d) => (
              <li key={d.path} className="flex flex-wrap items-center justify-between gap-3 py-3 first:pt-0 last:pb-0">
                <div className="min-w-0">
                  <p className="text-sm font-medium">{d.label}</p>
                  <p className="text-xs text-muted">{d.description}</p>
                </div>
                <button type="button" className="btn btn-sm" onClick={() => setParamsFor(d)} disabled={busy === d.path}>
                  {busy === d.path ? <Spinner className="size-3.5" /> : null}
                  Запустить
                </button>
              </li>
            ))}
          </ul>
        </Section>

        <Section id="mt-mig" icon={ShieldAlert} title="Миграции и исправления" description="Изменяют данные - перед запуском сделайте резервную копию">
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
                  Выполнить
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
