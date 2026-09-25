import { useCallback, useEffect, useRef, useState } from 'react'

/**
 * Load `fn()` on mount and every `intervalMs` while `enabled` and the tab is
 * visible. Returns `{ data, error, loading, reload }`.
 */
export function usePolling(fn, intervalMs, { enabled = true } = {}) {
  const [data, setData] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(true)
  const fnRef = useRef(fn)
  useEffect(() => {
    fnRef.current = fn
  })

  const reload = useCallback(async () => {
    try {
      const d = await fnRef.current()
      setData(d)
      setError(null)
    } catch (e) {
      if (e?.name !== 'AbortError') setError(e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    reload()
  }, [reload])

  useEffect(() => {
    if (!enabled || !intervalMs) return undefined
    const id = setInterval(() => {
      if (typeof document === 'undefined' || document.visibilityState !== 'hidden') reload()
    }, intervalMs)
    return () => clearInterval(id)
  }, [enabled, intervalMs, reload])

  return { data, error, loading, reload }
}
