import { Eye, EyeOff } from 'lucide-react'
import { useId, useState } from 'react'
import { useApp } from '../context.js'

/** Input + save/remove for the API key. Used in the header popover and inline on the search page. */
export function ApiKeyForm({ onSaved, autoFocus = false, compact = false }) {
  const { t, apiKey, setApiKey, conf } = useApp()
  const [draft, setDraft] = useState(apiKey)
  const [visible, setVisible] = useState(false)
  const id = useId()

  const status = !apiKey ? 'missing' : conf.loading ? '' : conf.valid ? 'valid' : 'invalid'

  function submit(e) {
    e.preventDefault()
    setApiKey(draft)
    onSaved?.()
  }

  return (
    <form onSubmit={submit} className="space-y-3">
      {!compact && <p className="text-sm text-muted">{t('apikey.hint')}</p>}
      <div>
        <label htmlFor={id} className="mb-1.5 block text-sm font-medium">
          {t('apikey.title')}
        </label>
        <div className="relative">
          <input
            id={id}
            type={visible ? 'text' : 'password'}
            className="input pr-10 font-mono"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={t('apikey.placeholder')}
            autoComplete="off"
            spellCheck={false}
            autoFocus={autoFocus}
            aria-describedby={`${id}-status`}
          />
          <button
            type="button"
            className="absolute inset-y-0 right-0 flex w-10 items-center justify-center text-faint hover:text-fg"
            onClick={() => setVisible((v) => !v)}
            aria-label={visible ? t('apikey.hide') : t('apikey.show')}
          >
            {visible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        </div>
        <p id={`${id}-status`} className="mt-1.5 flex items-center gap-1.5 text-xs text-muted">
          {status && (
            <>
              <span
                aria-hidden
                className={`size-1.5 rounded-full ${status === 'valid' ? 'bg-ok' : status === 'invalid' ? 'bg-red-500' : 'bg-faint'}`}
              />
              {t(`apikey.${status}`)}
            </>
          )}
        </p>
      </div>
      <div className="flex gap-2">
        <button type="submit" className="btn btn-primary">
          {t('apikey.save')}
        </button>
        {apiKey && (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => {
              setDraft('')
              setApiKey('')
            }}
          >
            {t('apikey.clear')}
          </button>
        )}
      </div>
    </form>
  )
}
