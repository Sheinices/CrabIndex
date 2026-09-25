import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

// Node >= 22 ships its own (file-backed) localStorage global that shadows jsdom's;
// install a plain in-memory Storage so tests behave the same on every Node version.
function memoryStorage() {
  let data = new Map()
  return {
    get length() {
      return data.size
    },
    key: (i) => [...data.keys()][i] ?? null,
    getItem: (k) => (data.has(String(k)) ? data.get(String(k)) : null),
    setItem: (k, v) => void data.set(String(k), String(v)),
    removeItem: (k) => void data.delete(String(k)),
    clear: () => void (data = new Map()),
  }
}
Object.defineProperty(globalThis, 'localStorage', { value: memoryStorage(), configurable: true, writable: true })

afterEach(() => {
  cleanup()
  localStorage.clear()
})
