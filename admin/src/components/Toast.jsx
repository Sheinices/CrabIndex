import { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react'
import { CheckCircle2, AlertTriangle, Info, X } from 'lucide-react'

const ToastContext = createContext(null)

const ICONS = { success: CheckCircle2, error: AlertTriangle, info: Info }
const TONES = { success: 'text-ok', error: 'text-danger', info: 'text-accent' }

export function ToastProvider({ children }) {
  const [items, setItems] = useState([])
  const seq = useRef(0)

  const dismiss = useCallback((id) => setItems((xs) => xs.filter((t) => t.id !== id)), [])

  const push = useCallback(
    (kind, title, detail, { timeout = kind === 'error' ? 8000 : 4500 } = {}) => {
      const id = ++seq.current
      setItems((xs) => [...xs.slice(-4), { id, kind, title, detail }])
      if (timeout) setTimeout(() => dismiss(id), timeout)
      return id
    },
    [dismiss],
  )

  const api = useMemo(
    () => ({
      success: (t, d, o) => push('success', t, d, o),
      error: (t, d, o) => push('error', t, d, o),
      info: (t, d, o) => push('info', t, d, o),
      dismiss,
    }),
    [push, dismiss],
  )

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div
        className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col items-center gap-2 p-4 sm:items-end"
        role="region"
        aria-label="Уведомления"
      >
        {items.map((t) => {
          const Icon = ICONS[t.kind] || Info
          return (
            <div
              key={t.id}
              role={t.kind === 'error' ? 'alert' : 'status'}
              className="pointer-events-auto flex w-full max-w-sm items-start gap-3 rounded-xl border border-border bg-surface p-3 shadow-lg"
            >
              <Icon className={`mt-0.5 size-5 shrink-0 ${TONES[t.kind]}`} aria-hidden="true" />
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">{t.title}</p>
                {t.detail ? <p className="mt-1 line-clamp-4 text-xs break-words whitespace-pre-wrap text-muted">{t.detail}</p> : null}
              </div>
              <button type="button" className="btn-ghost btn btn-sm -m-1 p-1" onClick={() => dismiss(t.id)} aria-label="Закрыть">
                <X className="size-4" aria-hidden="true" />
              </button>
            </div>
          )
        })}
      </div>
    </ToastContext.Provider>
  )
}

export function useToast() {
  const ctx = useContext(ToastContext)
  if (!ctx) throw new Error('useToast outside ToastProvider')
  return ctx
}
