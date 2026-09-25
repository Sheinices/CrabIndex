import { KeyRound } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useApp } from '../context.js'
import { ApiKeyForm } from './ApiKeyForm.jsx'

/** Small header control, rendered only when the server requires an API key. */
export function ApiKeyButton() {
  const { t, apiKey, conf } = useApp()
  const [open, setOpen] = useState(false)
  const wrap = useRef(null)
  const ok = !!apiKey && conf.valid

  useEffect(() => {
    if (!open) return undefined
    const onDown = (e) => {
      if (wrap.current && !wrap.current.contains(e.target)) setOpen(false)
    }
    const onKey = (e) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={wrap} className="relative">
      <button
        type="button"
        className="btn btn-ghost px-2.5"
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((v) => !v)}
        title={t('apikey.title')}
      >
        <span className="relative">
          <KeyRound className="size-4" aria-hidden />
          <span
            aria-hidden
            className={`absolute -top-0.5 -right-1 size-2 rounded-full ring-2 ring-bg ${ok ? 'bg-ok' : 'bg-red-500'}`}
          />
        </span>
        <span className="hidden md:inline">{t('apikey.button')}</span>
        <span className="sr-only">{ok ? t('apikey.valid') : apiKey ? t('apikey.invalid') : t('apikey.missing')}</span>
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t('apikey.title')}
          className="card absolute right-0 z-50 mt-2 w-[min(22rem,calc(100vw-2rem))] p-4 shadow-xl"
        >
          <ApiKeyForm autoFocus onSaved={() => setOpen(false)} />
        </div>
      )}
    </div>
  )
}
