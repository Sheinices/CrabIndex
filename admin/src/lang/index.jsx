// Lightweight i18n for the admin panel - no external dependency, in the spirit of the
// project. Built-in languages (ru, en) are compiled in; runtime packs from `Data/lang/*.json`
// are fetched from `GET /api/lang` and merged on top, so a new language can be added on the
// server without rebuilding the panel.
//
// Lookup order for a key: selected runtime pack -> selected built-in -> Russian base -> key.
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import meta from './meta.js'
import ru from './ru.js'
import en from './en.js'
import { getLang } from '../lib/api.js'

const BUILTIN = { ru, en }
const STORAGE_KEY = 'crab.lang'

/** Resolve one key against a dictionary and fill `{name}` placeholders. */
function translate(dict, key, vars) {
  let s = dict[key]
  if (s == null) s = key
  if (vars) for (const k in vars) s = String(s).split(`{${k}}`).join(vars[k])
  return s
}

/** Fallback used when a component calls useT() outside a provider: the Russian base. */
function baseT(key, vars) {
  return translate(ru, key, vars)
}

/** Same fallback for plain (non-hook) helpers that accept an optional `t`. */
export const tBase = baseT

/** First run: saved choice, else the browser's language if we have it, else Russian. */
function initialLang() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved) return saved
  } catch {
    /* storage blocked - fall through to detection */
  }
  const nav = (typeof navigator !== 'undefined' && navigator.language ? navigator.language : 'ru').slice(0, 2).toLowerCase()
  return nav
}

const LangContext = createContext(null)

export function LangProvider({ children }) {
  const [lang, setLangState] = useState(initialLang)
  // Runtime packs fetched from the server: code -> { name, strings }.
  const [packs, setPacks] = useState({})

  useEffect(() => {
    let alive = true
    getLang({ silent401: true })
      .then((res) => {
        if (!alive || !res?.languages) return
        const next = {}
        for (const l of res.languages) {
          if (l && l.code) next[l.code] = { name: l.name || l.code, strings: l.strings || {} }
        }
        setPacks(next)
      })
      .catch(() => {
        /* no packs / not reachable - built-in languages still work */
      })
    return () => {
      alive = false
    }
  }, [])

  // Full language menu: built-in first, then any runtime packs not shadowing a built-in.
  const languages = useMemo(() => {
    const out = {}
    for (const code in meta) out[code] = { code, name: meta[code].name }
    for (const code in packs) if (!out[code]) out[code] = { code, name: packs[code].name }
    return out
  }, [packs])

  // Active dictionary with fallback chain resolved once per language change.
  const dict = useMemo(() => {
    const base = BUILTIN.ru || {}
    const builtin = BUILTIN[lang] || {}
    const pack = packs[lang]?.strings || {}
    return { ...base, ...builtin, ...pack }
  }, [lang, packs])

  const t = useCallback((key, vars) => translate(dict, key, vars), [dict])

  const setLang = useCallback((code) => {
    try {
      localStorage.setItem(STORAGE_KEY, code)
    } catch {
      /* storage blocked - selection just won't persist */
    }
    setLangState(code)
    try {
      if (typeof document !== 'undefined') document.documentElement.lang = code
    } catch {
      /* ignore */
    }
  }, [])

  useEffect(() => {
    try {
      if (typeof document !== 'undefined') document.documentElement.lang = lang
    } catch {
      /* ignore */
    }
  }, [lang])

  const value = useMemo(() => ({ lang, setLang, t, languages }), [lang, setLang, t, languages])
  return <LangContext.Provider value={value}>{children}</LangContext.Provider>
}

/** Translate function `t(key, vars?)`. Outside a provider it falls back to the Russian base. */
export function useT() {
  const ctx = useContext(LangContext)
  return ctx ? ctx.t : baseT
}

/** Current language, setter and the available-languages map. */
export function useLang() {
  const ctx = useContext(LangContext)
  return ctx || { lang: 'ru', setLang: () => {}, languages: meta }
}
