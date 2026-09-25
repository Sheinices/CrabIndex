import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getConf } from './lib/api.js'
import { getApiKey, setApiKey as storeApiKey } from './lib/apikey.js'
import { createT, normalizeLocale } from './lib/i18n.js'
import { KEYS, readItem, writeItem } from './lib/storage.js'
import { AppContext } from './context.js'

function applyTheme(theme) {
  const root = document.documentElement
  root.classList.toggle('dark', theme === 'dark')
  root.dataset.theme = theme
  root.style.colorScheme = theme
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta) meta.setAttribute('content', theme === 'dark' ? '#0c0c0d' : '#f7f7f8')
}

export function AppProvider({ children, initialConf }) {
  const [locale, setLocaleState] = useState(() => normalizeLocale(readItem(KEYS.locale)))
  const [theme, setTheme] = useState(() => (readItem(KEYS.theme) === 'light' ? 'light' : 'dark'))
  const [apiKey, setApiKeyState] = useState(getApiKey)
  // conf: { loading, configured, valid, error }
  const [conf, setConf] = useState(initialConf || { loading: true, configured: false, valid: true, error: false })
  const [toast, setToast] = useState(null)
  const toastTimer = useRef(0)

  const t = useMemo(() => createT(locale), [locale])

  useEffect(() => {
    document.documentElement.lang = locale
  }, [locale])

  useEffect(() => {
    applyTheme(theme)
  }, [theme])

  useEffect(() => {
    if (initialConf) return undefined
    const ctrl = new AbortController()
    getConf(apiKey, { signal: ctrl.signal })
      .then((c) => setConf({ loading: false, configured: !!c?.configured, valid: c?.apikey !== false, error: false }))
      .catch((err) => {
        if (err?.name === 'AbortError') return
        setConf({ loading: false, configured: false, valid: true, error: true })
      })
    return () => ctrl.abort()
  }, [apiKey, initialConf])

  const setLocale = useCallback((next) => {
    const value = normalizeLocale(next)
    writeItem(KEYS.locale, value === 'ru' ? '' : value)
    setLocaleState(value)
  }, [])

  const toggleTheme = useCallback(() => {
    setTheme((prev) => {
      const next = prev === 'dark' ? 'light' : 'dark'
      writeItem(KEYS.theme, next)
      return next
    })
  }, [])

  const setApiKey = useCallback((value) => {
    storeApiKey(value)
    setApiKeyState(getApiKey())
  }, [])

  const showToast = useCallback((message, tone = 'ok') => {
    window.clearTimeout(toastTimer.current)
    setToast({ message, tone, id: Date.now() })
    toastTimer.current = window.setTimeout(() => setToast(null), 2600)
  }, [])

  const value = useMemo(
    () => ({ locale, setLocale, t, theme, toggleTheme, apiKey, setApiKey, conf, showToast }),
    [locale, setLocale, t, theme, toggleTheme, apiKey, setApiKey, conf, showToast],
  )

  return (
    <AppContext.Provider value={value}>
      {children}
      <div aria-live="polite" role="status" className="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex justify-center px-4">
        {toast && (
          <div
            key={toast.id}
            className={`toast-in pointer-events-auto rounded-xl px-4 py-2.5 text-sm font-medium shadow-lg ring-1 ${
              toast.tone === 'error' ? 'bg-red-600 text-white ring-red-500/40' : 'bg-fg text-bg ring-black/10'
            }`}
          >
            {toast.message}
          </div>
        )}
      </div>
    </AppContext.Provider>
  )
}
