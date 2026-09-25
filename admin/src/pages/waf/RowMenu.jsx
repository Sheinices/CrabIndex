import { useEffect, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { MoreHorizontal } from 'lucide-react'

/**
 * Compact per-row action menu. Rendered in a portal with fixed positioning so
 * it is not clipped by the scrolling table wrapper.
 * `items`: `[{ label, onSelect, danger?, hidden?, separator? }]`.
 */
export function RowMenu({ label, items }) {
  const [pos, setPos] = useState(null)
  const btnRef = useRef(null)
  const menuRef = useRef(null)
  const id = useId()

  useEffect(() => {
    if (!pos) return undefined
    const close = (e) => {
      if (e.type === 'keydown' && e.key !== 'Escape') return
      if (e.type === 'mousedown' && (menuRef.current?.contains(e.target) || btnRef.current?.contains(e.target))) return
      setPos(null)
      if (e.type === 'keydown') btnRef.current?.focus()
    }
    const onScroll = () => setPos(null)
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', close)
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onScroll)
    menuRef.current?.querySelector('button')?.focus()
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', close)
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onScroll)
    }
  }, [pos])

  const toggle = () => {
    if (pos) return setPos(null)
    const r = btnRef.current.getBoundingClientRect()
    const below = window.innerHeight - r.bottom > 280
    setPos({ right: Math.max(8, window.innerWidth - r.right), ...(below ? { top: r.bottom + 4 } : { bottom: window.innerHeight - r.top + 4 }) })
  }

  const onMenuKey = (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    e.preventDefault()
    const els = [...menuRef.current.querySelectorAll('button')]
    const i = els.indexOf(document.activeElement)
    const next = e.key === 'ArrowDown' ? (i + 1) % els.length : (i - 1 + els.length) % els.length
    els[next]?.focus()
  }

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        className="btn btn-ghost btn-sm"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={!!pos}
        aria-controls={pos ? id : undefined}
        onClick={toggle}
      >
        <MoreHorizontal className="size-4" aria-hidden="true" />
      </button>
      {pos
        ? createPortal(
            <div
              ref={menuRef}
              id={id}
              role="menu"
              aria-label={label}
              onKeyDown={onMenuKey}
              className="fixed z-40 min-w-52 rounded-xl border border-border bg-surface p-1 shadow-xl"
              style={pos}
            >
              {items
                .filter((it) => !it.hidden)
                .map((it, i) =>
                  it.separator ? (
                    <div key={`sep-${i}`} role="separator" className="my-1 border-t border-border" />
                  ) : (
                    <button
                      key={it.label}
                      type="button"
                      role="menuitem"
                      className={`flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left text-sm hover:bg-surface-2 focus:bg-surface-2 focus:outline-none ${
                        it.danger ? 'text-danger' : ''
                      }`}
                      onClick={() => {
                        setPos(null)
                        it.onSelect()
                      }}
                    >
                      {it.icon ? <it.icon className="size-4 shrink-0 opacity-70" aria-hidden="true" /> : null}
                      {it.label}
                    </button>
                  ),
                )}
            </div>,
            document.body,
          )
        : null}
    </>
  )
}
