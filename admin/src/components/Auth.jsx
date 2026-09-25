import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import * as apiClient from '../lib/api.js'

const AuthContext = createContext(null)

/** Session state: 'loading' | 'anon' | 'authed' | 'error'. */
export function AuthProvider({ children }) {
  const [status, setStatus] = useState('loading')
  const [session, setSession] = useState(null)
  const [error, setError] = useState(null)

  const refresh = useCallback(async () => {
    try {
      const s = await apiClient.getSession()
      setSession(s)
      setError(null)
      setStatus(s?.authenticated ? 'authed' : 'anon')
    } catch (e) {
      setError(e)
      setStatus(e?.status === 401 ? 'anon' : 'error')
    }
  }, [])

  useEffect(() => {
    refresh()
    return apiClient.onUnauthorized(() => setStatus('anon'))
  }, [refresh])

  const login = useCallback(async (devkey) => {
    await apiClient.login(devkey)
    setStatus('authed')
    try {
      setSession(await apiClient.getSession())
    } catch {
      /* session info is cosmetic */
    }
  }, [])

  const logout = useCallback(async () => {
    try {
      await apiClient.logout()
    } finally {
      setStatus('anon')
    }
  }, [])

  const value = useMemo(() => ({ status, session, error, login, logout, refresh }), [status, session, error, login, logout, refresh])
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth outside AuthProvider')
  return ctx
}
