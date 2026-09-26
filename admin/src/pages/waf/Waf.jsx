import { NavLink, Outlet, useOutletContext } from 'react-router'
import { Bot, Gauge, ListFilter, Network, ShieldCheck } from 'lucide-react'
import { getWafRules } from '../../lib/api.js'
import { usePolling } from '../../hooks/usePolling.js'
import { PageHeader } from '../../components/ui.jsx'
import { DisabledNotice } from './shared.jsx'

const TABS = [
  { to: '/waf', label: 'Обзор', icon: Gauge, end: true },
  { to: '/waf/log', label: 'Журнал', icon: ListFilter },
  { to: '/waf/ips', label: 'IP-адреса', icon: Network },
  { to: '/waf/rules', label: 'Правила', icon: ShieldCheck },
  { to: '/waf/bots', label: 'Боты', icon: Bot },
]

/** Shared WAF state for the sub-tabs: rules (lists, bans, config, `you`). */
export function useWaf() {
  return useOutletContext()
}

export function WafPage() {
  // Rules carry the config (enable flag) and the requester IP for self-protection.
  const rules = usePolling(() => getWafRules(), 30_000)
  const enabled = rules.data?.config ? rules.data.config.enable !== false : true

  return (
    <>
      <PageHeader title="WAF" description="Фильтрация запросов, баны и статистика трафика" />
      {!enabled ? <DisabledNotice /> : null}
      <nav aria-label="Разделы WAF" className="mb-6 flex gap-1 overflow-x-auto shadow-[inset_0_-1px_0_var(--border)]">
        {TABS.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              `inline-flex shrink-0 items-center gap-2 border-b-2 px-3 py-2 text-sm font-medium transition-colors ${
                isActive ? 'border-brand text-accent' : 'border-transparent text-muted hover:text-fg'
              }`
            }
          >
            <Icon className="size-4" aria-hidden="true" />
            {label}
          </NavLink>
        ))}
      </nav>
      <Outlet context={{ rules, enabled, you: rules.data?.you || null }} />
    </>
  )
}
