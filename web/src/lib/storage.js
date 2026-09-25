/** localStorage keys (names kept from earlier releases so saved values survive the upgrade). */
export const KEYS = {
  apiKey: 'api_key',
  theme: 'theme',
  locale: 'crabindexLocale',
  recent: 'crabindexRecentSearches',
}

export function readItem(key) {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

export function writeItem(key, value) {
  try {
    if (value == null || value === '') localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  } catch {
    /* private mode / quota */
  }
}
