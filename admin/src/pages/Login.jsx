import { useId, useState } from 'react'
import { Eye, EyeOff, KeyRound, LogIn } from 'lucide-react'
import { useAuth } from '../components/Auth.jsx'
import { getBase } from '../lib/base.js'
import { Spinner } from '../components/ui.jsx'
import { useT } from '../lang/index.jsx'

export function loginErrorText(err, t = (k) => k) {
  if (!err) return ''
  if (err.status === 401) return t('login_err_invalid')
  if (err.status === 429) return t('login_err_ratelimit')
  if (err.status === 0) return t('login_err_unreachable')
  return err.message || t('login_err_generic')
}

export function LoginPage() {
  const { login } = useAuth()
  const t = useT()
  const [devkey, setDevkey] = useState('')
  const [show, setShow] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const inputId = useId()
  const errId = useId()
  const hintId = useId()

  const submit = async (e) => {
    e.preventDefault()
    if (!devkey.trim() || busy) return
    setBusy(true)
    setError(null)
    try {
      await login(devkey.trim())
    } catch (err) {
      setError(err)
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="grid min-h-screen place-items-center px-4 py-10">
      <div className="w-full max-w-sm">
        <div className="mb-8 flex flex-col items-center gap-3 text-center">
          <img src={`${getBase()}/icon-192.png`} alt="" width="80" height="80" className="size-20" />
          <div>
            <h1 className="text-2xl font-extrabold tracking-tight">
              <span className="text-accent">Crab</span>
              <span>Index</span>
            </h1>
            <p className="text-sm text-muted">{t('login_subtitle')}</p>
          </div>
        </div>
        <form onSubmit={submit} className="card space-y-4 p-6" noValidate>
          <div>
            <label htmlFor={inputId} className="label">
              {t('login_devkey')}
            </label>
            <div className="relative">
              <KeyRound className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
              <input
                id={inputId}
                type={show ? 'text' : 'password'}
                className="input pr-10 pl-9 font-mono"
                autoComplete="current-password"
                autoFocus
                value={devkey}
                onChange={(e) => setDevkey(e.target.value)}
                aria-invalid={!!error}
                aria-describedby={`${error ? errId : ''} ${hintId}`.trim()}
                spellCheck={false}
              />
              <button
                type="button"
                className="absolute top-1/2 right-1.5 -translate-y-1/2 rounded-md p-1.5 text-muted hover:text-fg"
                onClick={() => setShow((s) => !s)}
                aria-label={show ? t('login_hide_key') : t('login_show_key')}
                aria-pressed={show}
              >
                {show ? <EyeOff className="size-4" aria-hidden="true" /> : <Eye className="size-4" aria-hidden="true" />}
              </button>
            </div>
          </div>
          {error ? (
            <p id={errId} role="alert" className="rounded-lg bg-danger/10 px-3 py-2 text-sm text-danger">
              {loginErrorText(error, t)}
            </p>
          ) : null}
          <button type="submit" className="btn btn-primary w-full" disabled={busy || !devkey.trim()}>
            {busy ? <Spinner /> : <LogIn className="size-4" aria-hidden="true" />}
            {t('login_submit')}
          </button>
          <p id={hintId} className="text-xs leading-relaxed text-muted">
            {t('login_hint_prefix')} <code className="font-mono text-fg">devkey</code> {t('login_hint_from')}{' '}
            <code className="font-mono text-fg">init.yaml</code>. {t('login_hint_suffix')}{' '}
            <code className="font-mono text-fg">crabindex admin</code>.
          </p>
        </form>
      </div>
    </main>
  )
}
