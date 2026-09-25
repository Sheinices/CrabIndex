import { Search, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useT } from '../context.js'
import { queryKind } from '../lib/torrents.js'

function isTypingTarget(el) {
  if (!el) return false
  const tag = el.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable
}

/** Big search box. Pressing "/" anywhere on the page focuses it. */
export function SearchForm({ query, onSearch, large = false, loading = false }) {
  const t = useT()
  const [draft, setDraft] = useState(query)
  const [synced, setSynced] = useState(query)
  const input = useRef(null)

  // Follow the URL (back/forward, clicking a recent search).
  if (synced !== query) {
    setSynced(query)
    setDraft(query)
  }

  useEffect(() => {
    const onKey = (e) => {
      if (e.key !== '/' || e.ctrlKey || e.metaKey || e.altKey || isTypingTarget(e.target)) return
      e.preventDefault()
      input.current?.focus()
      input.current?.select()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [])

  const kind = queryKind(draft)

  return (
    <form
      role="search"
      onSubmit={(e) => {
        e.preventDefault()
        onSearch(draft.trim())
        input.current?.blur()
      }}
    >
      <label htmlFor="q" className="sr-only">
        {t('search.label')}
      </label>
      <div
        className={`group flex items-center gap-2 rounded-2xl border border-line bg-surface pr-2 pl-4 shadow-sm transition-colors focus-within:border-brand ${
          large ? 'h-16' : 'h-13'
        }`}
      >
        <Search className={`shrink-0 text-faint ${large ? 'size-5' : 'size-[18px]'}`} aria-hidden />
        <input
          ref={input}
          id="q"
          name="q"
          type="search"
          enterKeyHint="search"
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          autoFocus={large}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape' && draft) {
              e.preventDefault()
              setDraft('')
            }
          }}
          placeholder={t('search.placeholder')}
          className={`h-full min-w-0 flex-1 bg-transparent text-fg placeholder:text-faint focus:outline-none ${
            large ? 'text-lg' : 'text-base'
          }`}
        />
        {draft ? (
          <button
            type="button"
            className="icon-btn size-8"
            onClick={() => {
              setDraft('')
              input.current?.focus()
            }}
            aria-label={t('search.clear')}
          >
            <X className="size-4" />
          </button>
        ) : (
          <kbd
            className="hidden rounded-md border border-line px-1.5 py-0.5 font-mono text-xs text-faint sm:inline"
            title={t('search.shortcut')}
          >
            /
          </kbd>
        )}
        <button type="submit" className={`btn btn-primary ${large ? 'h-11 px-5' : 'h-9 px-4'}`} disabled={loading && draft.trim() === query}>
          <span className="hidden sm:inline">{t('search.submit')}</span>
          <Search className="size-4 sm:hidden" aria-hidden />
          <span className="sr-only sm:hidden">{t('search.submit')}</span>
        </button>
      </div>
      {kind !== 'text' && <p className="mt-2 pl-4 text-xs text-muted">{t(`search.kind.${kind}`)}</p>}
    </form>
  )
}
