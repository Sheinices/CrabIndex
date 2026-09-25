import { useSyncExternalStore } from 'react'

const KEY = 'crab-admin-theme'
const listeners = new Set()

function readStored() {
  try {
    const t = localStorage.getItem(KEY)
    if (t === 'light' || t === 'dark') return t
  } catch {
    /* storage unavailable */
  }
  return 'dark'
}

let current = null

function getTheme() {
  if (current == null) current = readStored()
  return current
}

export function setTheme(theme) {
  current = theme === 'light' ? 'light' : 'dark'
  document.documentElement.dataset.theme = current
  try {
    localStorage.setItem(KEY, current)
  } catch {
    /* storage unavailable */
  }
  listeners.forEach((fn) => fn())
}

export function applyStoredTheme() {
  document.documentElement.dataset.theme = getTheme()
}

function subscribe(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

/** Shared theme (dark by default) with a light toggle, persisted per browser. */
export function useTheme() {
  const theme = useSyncExternalStore(subscribe, getTheme, () => 'dark')
  return { theme, toggle: () => setTheme(theme === 'dark' ? 'light' : 'dark') }
}
