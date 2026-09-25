import { useEffect, useState } from 'react'
import { searchTorrents } from '../lib/api.js'

/**
 * Fetches results for `query`. Returns { status, items, error, fetchedAt, retry } where
 * status is 'idle' | 'loading' | 'done' | 'error'.
 */
export function useTorrentSearch(query, apiKey, { onSuccess } = {}) {
  const q = String(query || '').trim()
  const [attempt, setAttempt] = useState(0)
  const [state, setState] = useState({ key: '', status: 'idle', items: [], error: null })
  const key = `${q}\u0000${apiKey}\u0000${attempt}`

  useEffect(() => {
    if (q.length < 2) return undefined
    const ctrl = new AbortController()
    searchTorrents(q, apiKey, { signal: ctrl.signal })
      .then((items) => {
        setState({ key, status: 'done', items, error: null, fetchedAt: Date.now() })
        onSuccess?.(q, items)
      })
      .catch((error) => {
        if (error?.name === 'AbortError') return
        setState({ key, status: 'error', items: [], error })
      })
    return () => ctrl.abort()
    // onSuccess is intentionally not a dependency: it must not re-trigger the request.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])

  const retry = () => setAttempt((n) => n + 1)
  if (q.length < 2) return { status: 'idle', items: [], error: null, retry }
  if (state.key !== key) return { status: 'loading', items: [], error: null, retry }
  return { ...state, retry }
}
