import { BookOpen, Braces, ChartColumn, Menu, Moon, Search, Sun, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import { useApp } from '../context.js'
import { ApiKeyButton } from './ApiKeyButton.jsx'
import { Footer } from './Footer.jsx'

const navClass = ({ isActive }) =>
  `inline-flex items-center gap-2 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
    isActive ? 'bg-surface-2 text-fg' : 'text-muted hover:bg-surface-2 hover:text-fg'
  }`

function NavItems({ onNavigate }) {
  const { t } = useApp()
  return (
    <>
      <NavLink to="/" end className={navClass} onClick={onNavigate}>
        <Search className="size-4" aria-hidden />
        {t('nav.search')}
      </NavLink>
      <NavLink to="/stats" className={navClass} onClick={onNavigate}>
        <ChartColumn className="size-4" aria-hidden />
        {t('nav.stats')}
      </NavLink>
      <a href="/docs/" className={navClass({ isActive: false })}>
        <BookOpen className="size-4" aria-hidden />
        {t('nav.docs')}
      </a>
      <a href="/swagger/" className={navClass({ isActive: false })}>
        <Braces className="size-4" aria-hidden />
        {t('nav.api')}
      </a>
    </>
  )
}

export function Layout() {
  const { t, theme, toggleTheme, locale, setLocale, conf } = useApp()
  const [menuOpen, setMenuOpen] = useState(false)
  const location = useLocation()
  const [menuPath, setMenuPath] = useState(location.pathname)

  // Close the mobile menu after navigation.
  if (menuPath !== location.pathname) {
    setMenuPath(location.pathname)
    setMenuOpen(false)
  }

  useEffect(() => {
    if (!menuOpen) return undefined
    const onKey = (e) => e.key === 'Escape' && setMenuOpen(false)
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [menuOpen])

  return (
    <div className="flex min-h-dvh flex-col">
      <a
        href="#main"
        className="sr-only z-50 rounded-lg bg-brand px-3 py-2 text-brand-fg focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        {t('skip')}
      </a>
      <header className="sticky top-0 z-30 border-b border-line bg-bg/85 backdrop-blur-md">
        <div className="mx-auto flex h-14 max-w-7xl items-center gap-2 px-4 sm:px-6">
          <NavLink to="/" className="mr-2 flex shrink-0 items-center gap-2.5 rounded-lg" aria-label="CrabIndex">
            <img src="/img/icon-192.png" alt="" width="28" height="28" className="size-7" />
            <span className="text-[15px] font-semibold tracking-tight">CrabIndex</span>
          </NavLink>
          <nav aria-label="Main" className="hidden items-center gap-1 md:flex">
            <NavItems />
          </nav>
          <div className="ml-auto flex items-center gap-1">
            {conf.configured && <ApiKeyButton />}
            <button
              type="button"
              className="icon-btn w-auto px-2 text-xs font-semibold"
              onClick={() => setLocale(locale === 'ru' ? 'en' : 'ru')}
              aria-label={t('lang.switch')}
              title={t('lang.switch')}
            >
              {t('lang.short')}
            </button>
            <button
              type="button"
              className="icon-btn"
              onClick={toggleTheme}
              aria-label={theme === 'dark' ? t('theme.toLight') : t('theme.toDark')}
              title={theme === 'dark' ? t('theme.toLight') : t('theme.toDark')}
            >
              {theme === 'dark' ? <Sun className="size-4" /> : <Moon className="size-4" />}
            </button>
            <button
              type="button"
              className="icon-btn md:hidden"
              aria-label={t('nav.menu')}
              aria-expanded={menuOpen}
              aria-controls="mobile-nav"
              onClick={() => setMenuOpen((v) => !v)}
            >
              {menuOpen ? <X className="size-5" /> : <Menu className="size-5" />}
            </button>
          </div>
        </div>
        {menuOpen && (
          <nav id="mobile-nav" aria-label="Main" className="grid gap-1 border-t border-line px-4 py-3 md:hidden">
            <NavItems onNavigate={() => setMenuOpen(false)} />
          </nav>
        )}
      </header>
      <main id="main" className="flex-1">
        <Outlet />
      </main>
      <Footer />
    </div>
  )
}
