import { render } from '@testing-library/react'
import { useEffect } from 'react'
import { MemoryRouter, useLocation } from 'react-router'
import { vi } from 'vitest'
import { App } from '../App.jsx'
import { AppProvider } from '../AppProvider.jsx'

/** Latest router location, for URL assertions. */
export const locationRef = { current: null }

function LocationSpy() {
  const location = useLocation()
  useEffect(() => {
    locationRef.current = location
  }, [location])
  return null
}

/** Mocks fetch with a `(url) => [status, body]` router and records calls. */
export function mockServer(route) {
  const calls = []
  const fn = vi.fn(async (input) => {
    const url = new URL(String(input), 'http://localhost')
    calls.push(url)
    const [status, body] = route(url)
    return { ok: status >= 200 && status < 300, status, json: async () => body }
  })
  vi.stubGlobal('fetch', fn)
  return calls
}

export function renderApp(path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <AppProvider>
        <LocationSpy />
        <App />
      </AppProvider>
    </MemoryRouter>,
  )
}
