import { KEYS, readItem, writeItem } from './storage.js'

export function getApiKey() {
  return (readItem(KEYS.apiKey) || '').trim()
}

export function setApiKey(value) {
  writeItem(KEYS.apiKey, String(value || '').trim())
}
