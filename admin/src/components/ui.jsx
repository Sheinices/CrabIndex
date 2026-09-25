import { Loader2 } from 'lucide-react'

export function Spinner({ className = 'size-4', label }) {
  return (
    <span role={label ? 'status' : undefined} className="inline-flex items-center gap-2">
      <Loader2 className={`${className} animate-spin`} aria-hidden="true" />
      {label ? <span className="text-sm text-muted">{label}</span> : null}
    </span>
  )
}

export function ProgressBar({ value, label }) {
  const indeterminate = value == null
  return (
    <div
      className="h-2 w-full overflow-hidden rounded-full bg-surface-2"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : value}
    >
      <div
        className={`h-full rounded-full bg-brand transition-[width] duration-500 ${indeterminate ? 'w-1/3 animate-pulse' : ''}`}
        style={indeterminate ? undefined : { width: `${value}%` }}
      />
    </div>
  )
}

export function PageHeader({ title, description, actions }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold tracking-tight sm:text-2xl">{title}</h1>
        {description ? <p className="mt-1 text-sm text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </div>
  )
}

export function ErrorBox({ error, onRetry }) {
  if (!error) return null
  return (
    <div role="alert" className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-danger/40 bg-danger/10 px-4 py-3 text-sm text-danger">
      <span>{error.message || String(error)}</span>
      {onRetry ? (
        <button type="button" className="btn btn-sm" onClick={onRetry}>
          Повторить
        </button>
      ) : null}
    </div>
  )
}

export function StatusDot({ tone = 'muted', label }) {
  const colors = { ok: 'bg-ok', warn: 'bg-warn', danger: 'bg-danger', muted: 'bg-muted', brand: 'bg-brand' }
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`size-2 rounded-full ${colors[tone]}`} aria-hidden="true" />
      {label ? <span>{label}</span> : null}
    </span>
  )
}

export function Toggle({ checked, onChange, label, id, disabled }) {
  return (
    <label htmlFor={id} className="inline-flex cursor-pointer items-center gap-2 text-sm select-none">
      <span className="relative inline-flex">
        <input id={id} type="checkbox" className="peer sr-only" checked={!!checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
        <span className="h-5 w-9 rounded-full bg-surface-2 ring-1 ring-border transition-colors peer-checked:bg-brand peer-focus-visible:outline-2 peer-focus-visible:outline-brand" />
        <span className="absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition-transform peer-checked:translate-x-4" />
      </span>
      {label}
    </label>
  )
}
