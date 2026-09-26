import { useEffect, useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import {
  Activity,
  ArrowUpCircle,
  Cloud,
  ExternalLink,
  FileText,
  Languages,
  LayoutDashboard,
  LogOut,
  Menu,
  Moon,
  Radar,
  Settings,
  Shield,
  Sun,
  Wrench,
  X,
} from 'lucide-react'
import { useAuth } from './Auth.jsx'
import { useTheme } from '../hooks/useTheme.js'
import { getBase } from '../lib/base.js'
import { getUpdate } from '../lib/api.js'
import { useLang, useT } from '../lang/index.jsx'

export const NAV = [
  { to: '/', labelKey: 'nav_overview', icon: LayoutDashboard, end: true },
  { to: '/trackers', labelKey: 'nav_trackers', icon: Radar },
  { to: '/jobs', labelKey: 'nav_jobs', icon: Activity },
  { to: '/cloudflare', labelKey: 'nav_flaresolverr', icon: Cloud },
  { to: '/settings', labelKey: 'nav_settings', icon: Settings },
  { to: '/waf', labelKey: 'nav_waf', icon: Shield },
  { to: '/maintenance', labelKey: 'nav_maintenance', icon: Wrench },
  { to: '/logs', labelKey: 'nav_logs', icon: FileText },
  { to: '/update', labelKey: 'nav_update', icon: ArrowUpCircle },
]

export function Logo({ className = 'size-8' }) {
  return <img src={`${getBase()}/icon-192.png`} alt="" className={`${className} rounded-lg`} width="32" height="32" />
}

function NavItems({ onNavigate, updateAvailable }) {
  const t = useT()
  return (
    <ul className="space-y-1">
      {NAV.map(({ to, labelKey, icon: Icon, end }) => (
        <li key={to}>
          <NavLink
            to={to}
            end={end}
            onClick={onNavigate}
            className={({ isActive }) =>
              `flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                isActive ? 'bg-brand/15 text-accent' : 'text-muted hover:bg-surface-2 hover:text-fg'
              }`
            }
          >
            <Icon className="size-4 shrink-0" aria-hidden="true" />
            {t(labelKey)}
            {to === '/update' && updateAvailable ? (
              <span className="ml-auto size-2 rounded-full bg-warn" role="img" aria-label={t('update_available')} title={t('update_available')} />
            ) : null}
          </NavLink>
        </li>
      ))}
    </ul>
  )
}

/** Compact language picker. Lists built-in languages plus any runtime packs from the server. */
function LanguagePicker() {
  const t = useT()
  const { lang, setLang, languages } = useLang()
  const codes = Object.keys(languages)
  if (codes.length < 2) return null
  return (
    <label className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted">
      <Languages className="size-4 shrink-0" aria-hidden="true" />
      <span className="sr-only">{t('language')}</span>
      <select
        value={codes.includes(lang) ? lang : 'ru'}
        onChange={(e) => setLang(e.target.value)}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none"
        aria-label={t('language')}
      >
        {codes.map((code) => (
          <option key={code} value={code} className="bg-surface text-fg">
            {languages[code].name}
          </option>
        ))}
      </select>
    </label>
  )
}

export function Layout() {
  const { logout, session } = useAuth()
  const { theme, toggle } = useTheme()
  const t = useT()
  const [menuOpen, setMenuOpen] = useState(false)
  const [updateAvailable, setUpdateAvailable] = useState(false)
  const location = useLocation()

  // Reflect the server's cached GitHub check: once on load, then every 30 minutes so a
  // release that lands while the panel stays open lights up the indicator on its own. The
  // server refreshes that cache in the background, so this never hits GitHub directly.
  useEffect(() => {
    let alive = true
    const check = () =>
      getUpdate(false, { silent401: true })
        .then((r) => alive && setUpdateAvailable(Boolean(r?.available)))
        .catch(() => {})
    check()
    const id = setInterval(check, 30 * 60 * 1000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [])

  useEffect(() => {
    const current = NAV.find((n) => (n.end ? location.pathname === n.to : location.pathname.startsWith(n.to)))
    document.title = `${current ? `${t(current.labelKey)} · ` : ''}${t('app_title_suffix')}`
  }, [location.pathname, t])

  const sidebar = (onNavigate) => (
    <nav aria-label={t('nav_sections')} className="flex h-full flex-col gap-6 p-4">
      <div className="flex items-center gap-3 px-2">
        <Logo />
        <div className="min-w-0">
          <p className="font-semibold leading-tight">CrabIndex</p>
          <p className="text-xs text-muted">{t('app_subtitle')}{session?.version ? ` · ${session.version}` : ''}</p>
        </div>
      </div>
      <NavItems updateAvailable={updateAvailable} onNavigate={onNavigate} />
      <div className="mt-auto space-y-1 border-t border-border pt-4">
        <a href="/" className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-fg" target="_blank" rel="noopener">
          <ExternalLink className="size-4" aria-hidden="true" /> {t('open_site')}
        </a>
        <button
          type="button"
          onClick={toggle}
          className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-fg"
        >
          {theme === 'dark' ? <Sun className="size-4" aria-hidden="true" /> : <Moon className="size-4" aria-hidden="true" />}
          {theme === 'dark' ? t('theme_light') : t('theme_dark')}
        </button>
        <LanguagePicker />
        <button
          type="button"
          onClick={logout}
          className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-danger"
        >
          <LogOut className="size-4" aria-hidden="true" /> {t('logout')}
        </button>
      </div>
    </nav>
  )

  return (
    <div className="min-h-screen lg:flex">
      <a href="#main" className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 btn">
        {t('skip_to_content')}
      </a>
      <aside className="sticky top-0 hidden h-screen w-64 shrink-0 border-r border-border bg-surface lg:block">{sidebar()}</aside>

      <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-border bg-surface/90 px-4 py-3 backdrop-blur lg:hidden">
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => setMenuOpen(true)} aria-label={t('menu')} aria-expanded={menuOpen}>
          <Menu className="size-5" aria-hidden="true" />
        </button>
        <Logo className="size-7" />
        <span className="font-semibold">CrabIndex</span>
        <button type="button" className="btn btn-ghost btn-sm ml-auto" onClick={toggle} aria-label={t('toggle_theme')}>
          {theme === 'dark' ? <Sun className="size-4" aria-hidden="true" /> : <Moon className="size-4" aria-hidden="true" />}
        </button>
      </header>

      {menuOpen ? (
        <div className="fixed inset-0 z-40 lg:hidden">
          <div className="absolute inset-0 bg-black/60" onClick={() => setMenuOpen(false)} aria-hidden="true" />
          <div className="relative h-full w-72 max-w-[85vw] border-r border-border bg-surface">
            <button type="button" className="btn btn-ghost btn-sm absolute top-4 right-3" onClick={() => setMenuOpen(false)} aria-label={t('close_menu')}>
              <X className="size-4" aria-hidden="true" />
            </button>
            {sidebar(() => setMenuOpen(false))}
          </div>
        </div>
      ) : null}

      <main id="main" className="min-w-0 flex-1 px-4 py-6 sm:px-6 lg:px-10 lg:py-8">
        <div className="mx-auto max-w-7xl">
          <Outlet />
        </div>
      </main>
    </div>
  )
}
