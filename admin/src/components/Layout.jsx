import { useEffect, useState } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import {
  Activity,
  ArrowUpCircle,
  Cloud,
  ExternalLink,
  FileText,
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

export const NAV = [
  { to: '/', label: 'Обзор', icon: LayoutDashboard, end: true },
  { to: '/trackers', label: 'Трекеры', icon: Radar },
  { to: '/jobs', label: 'Задачи', icon: Activity },
  { to: '/cloudflare', label: 'FlareSolverr', icon: Cloud },
  { to: '/settings', label: 'Настройки', icon: Settings },
  { to: '/waf', label: 'WAF', icon: Shield },
  { to: '/maintenance', label: 'Обслуживание', icon: Wrench },
  { to: '/logs', label: 'Логи', icon: FileText },
  { to: '/update', label: 'Обновление', icon: ArrowUpCircle },
]

export function Logo({ className = 'size-8' }) {
  return <img src={`${getBase()}/icon-192.png`} alt="" className={`${className} rounded-lg`} width="32" height="32" />
}

function NavItems({ onNavigate, updateAvailable }) {
  return (
    <ul className="space-y-1">
      {NAV.map(({ to, label, icon: Icon, end }) => (
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
            {label}
            {to === '/update' && updateAvailable ? (
              <span className="ml-auto size-2 rounded-full bg-warn" role="img" aria-label="доступна новая версия" title="Доступна новая версия" />
            ) : null}
          </NavLink>
        </li>
      ))}
    </ul>
  )
}

export function Layout() {
  const { logout, session } = useAuth()
  const { theme, toggle } = useTheme()
  const [menuOpen, setMenuOpen] = useState(false)
  const [updateAvailable, setUpdateAvailable] = useState(false)
  const location = useLocation()

  // One check per panel load; the server caches the GitHub answer for 6 hours.
  useEffect(() => {
    let alive = true
    getUpdate(false, { silent401: true })
      .then((r) => alive && setUpdateAvailable(Boolean(r?.available)))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  useEffect(() => {
    const current = NAV.find((n) => (n.end ? location.pathname === n.to : location.pathname.startsWith(n.to)))
    document.title = `${current ? `${current.label} · ` : ''}CrabIndex - админ-панель`
  }, [location.pathname])

  const sidebar = (onNavigate) => (
    <nav aria-label="Разделы" className="flex h-full flex-col gap-6 p-4">
      <div className="flex items-center gap-3 px-2">
        <Logo />
        <div className="min-w-0">
          <p className="font-semibold leading-tight">CrabIndex</p>
          <p className="text-xs text-muted">Админ-панель{session?.version ? ` · ${session.version}` : ''}</p>
        </div>
      </div>
      <NavItems updateAvailable={updateAvailable} onNavigate={onNavigate} />
      <div className="mt-auto space-y-1 border-t border-border pt-4">
        <a href="/" className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-fg" target="_blank" rel="noopener">
          <ExternalLink className="size-4" aria-hidden="true" /> Открыть сайт
        </a>
        <button
          type="button"
          onClick={toggle}
          className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-fg"
        >
          {theme === 'dark' ? <Sun className="size-4" aria-hidden="true" /> : <Moon className="size-4" aria-hidden="true" />}
          {theme === 'dark' ? 'Светлая тема' : 'Тёмная тема'}
        </button>
        <button
          type="button"
          onClick={logout}
          className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-danger"
        >
          <LogOut className="size-4" aria-hidden="true" /> Выйти
        </button>
      </div>
    </nav>
  )

  return (
    <div className="min-h-screen lg:flex">
      <a href="#main" className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 btn">
        К содержимому
      </a>
      <aside className="sticky top-0 hidden h-screen w-64 shrink-0 border-r border-border bg-surface lg:block">{sidebar()}</aside>

      <header className="sticky top-0 z-30 flex items-center gap-3 border-b border-border bg-surface/90 px-4 py-3 backdrop-blur lg:hidden">
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => setMenuOpen(true)} aria-label="Меню" aria-expanded={menuOpen}>
          <Menu className="size-5" aria-hidden="true" />
        </button>
        <Logo className="size-7" />
        <span className="font-semibold">CrabIndex</span>
        <button type="button" className="btn btn-ghost btn-sm ml-auto" onClick={toggle} aria-label="Сменить тему">
          {theme === 'dark' ? <Sun className="size-4" aria-hidden="true" /> : <Moon className="size-4" aria-hidden="true" />}
        </button>
      </header>

      {menuOpen ? (
        <div className="fixed inset-0 z-40 lg:hidden">
          <div className="absolute inset-0 bg-black/60" onClick={() => setMenuOpen(false)} aria-hidden="true" />
          <div className="relative h-full w-72 max-w-[85vw] border-r border-border bg-surface">
            <button type="button" className="btn btn-ghost btn-sm absolute top-4 right-3" onClick={() => setMenuOpen(false)} aria-label="Закрыть меню">
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
