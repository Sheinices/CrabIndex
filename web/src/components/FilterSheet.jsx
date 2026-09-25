import { X } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { useT } from '../context.js'

/** Bottom sheet for filters on small screens. */
export function FilterSheet({ open, onClose, onReset, resultCount, children }) {
  const t = useT()
  const closeBtn = useRef(null)
  const onCloseRef = useRef(onClose)

  useEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    if (!open) return undefined
    const prevOverflow = document.body.style.overflow
    const prevFocus = document.activeElement
    document.body.style.overflow = 'hidden'
    closeBtn.current?.focus()
    const onKey = (e) => e.key === 'Escape' && onCloseRef.current()
    document.addEventListener('keydown', onKey)
    return () => {
      document.body.style.overflow = prevOverflow
      document.removeEventListener('keydown', onKey)
      prevFocus?.focus?.()
    }
  }, [open])

  if (!open) return null

  return (
    <div className="fixed inset-0 z-40 lg:hidden">
      <div className="absolute inset-0 bg-black/50" onClick={onClose} aria-hidden />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="filter-sheet-title"
        className="sheet-in absolute inset-x-0 bottom-0 flex max-h-[88dvh] flex-col rounded-t-2xl border-t border-line bg-surface shadow-2xl"
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 id="filter-sheet-title" className="text-base font-semibold">
            {t('filters.title')}
          </h2>
          <button ref={closeBtn} type="button" className="icon-btn" onClick={onClose} aria-label={t('filters.close')}>
            <X className="size-5" />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto overscroll-contain px-4 py-4">{children}</div>
        <div className="flex gap-2 border-t border-line px-4 pt-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
          <button type="button" className="btn btn-outline flex-1" onClick={onReset}>
            {t('filters.reset')}
          </button>
          <button type="button" className="btn btn-primary flex-[2]" onClick={onClose}>
            {t('filters.apply', { n: resultCount })}
          </button>
        </div>
      </div>
    </div>
  )
}
