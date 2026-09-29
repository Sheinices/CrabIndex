// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { ArrowDown, Ban, Clock, ListFilter, RefreshCw, Search, ShieldCheck, ShieldX, Unlock } from 'lucide-react'
import { addWafRule, banWafIp, getWafIps, unbanWafIp } from '../../lib/api.js'
import { usePolling } from '../../hooks/usePolling.js'
import { useConfirm } from '../../components/Confirm.jsx'
import { ErrorBox, Spinner } from '../../components/ui.jsx'
import { formatDate, formatNumber, formatRelative } from '../../lib/format.js'
import { BAN_PRESETS } from '../../lib/waf.js'
import { BanDialog, Empty, IpStateBadge, RuleDialog, useWafAction } from './shared.jsx'
import { RowMenu } from './RowMenu.jsx'
import { useWaf } from './Waf.jsx'
import { useT } from '../../lang/index.jsx'

// [sort id, i18n key]
const SORTS = [
  ['requests', 'requests'],
  ['blocked', 'blocked_short'],
  ['lastSeen', 'last_seen'],
]

function SortHeader({ id, label, sort, onSort, className = '' }) {
  const active = sort === id
  return (
    <th className={className} aria-sort={active ? 'descending' : 'none'}>
      <button type="button" onClick={() => onSort(id)} className={`inline-flex items-center gap-1 uppercase ${active ? 'text-accent' : 'hover:text-fg'}`}>
        {label}
        <ArrowDown className={`size-3 ${active ? '' : 'opacity-0'}`} aria-hidden="true" />
      </button>
    </th>
  )
}

export function WafIps() {
  const t = useT()
  const { you, rules } = useWaf()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const run = useWafAction()
  const [sort, setSort] = useState('requests')
  const [filter, setFilter] = useState('')
  const [dialog, setDialog] = useState(null) // { kind: 'blacklist'|'whitelist'|'ban', ip }
  const { data, error, loading, reload } = usePolling(() => getWafIps({ sort, limit: 200 }), 15_000)

  useEffect(() => {
    reload()
  }, [sort, reload])

  const rows = useMemo(() => {
    const arr = Array.isArray(data) ? data : Array.isArray(data?.ips) ? data.ips : []
    const q = filter.trim().toLowerCase()
    const key = sort === 'lastSeen' ? (r) => new Date(r.lastSeen).getTime() || 0 : (r) => Number(r[sort]) || 0
    return arr.filter((r) => !q || String(r.ip).toLowerCase().includes(q) || String(r.ua || '').toLowerCase().includes(q)).sort((a, b) => key(b) - key(a))
  }, [data, filter, sort])

  const refreshAll = async () => {
    await reload()
    await rules.reload()
  }

  const ban = (ip, minutes, label) => run(() => banWafIp({ ip, minutes, reason: 'manual' }), t('waf_banned_for', { ip, label }), refreshAll)

  const unban = async (ip) => {
    const ok = await confirm({
      title: t('waf_unban_q'),
      message: (
        <p>
          IP <span className="font-mono">{ip}</span> {t('waf_unban_msg_ips')}
        </p>
      ),
      confirmLabel: t('waf_unban_btn2'),
    })
    if (ok) await run(() => unbanWafIp(ip), t('waf_ban_lifted', { ip }), refreshAll)
  }

  const menu = (r) => [
    { label: t('waf_show_requests'), icon: ListFilter, onSelect: () => navigate(`/waf/log?ip=${encodeURIComponent(r.ip)}`) },
    { separator: true },
    { label: t('waf_block_ellipsis'), icon: ShieldX, danger: true, hidden: r.state === 'blacklisted', onSelect: () => setDialog({ kind: 'blacklist', ip: r.ip }) },
    { label: t('waf_whitelist_ellipsis'), icon: ShieldCheck, hidden: r.state === 'whitelisted', onSelect: () => setDialog({ kind: 'whitelist', ip: r.ip }) },
    { separator: true },
    ...BAN_PRESETS.map((p) => ({ label: t('waf_ban_for', { label: p.label }), icon: Clock, onSelect: () => ban(r.ip, p.minutes, p.label) })),
    { label: t('waf_ban_custom_ellipsis'), icon: Ban, onSelect: () => setDialog({ kind: 'ban', ip: r.ip }) },
    { label: t('waf_unban_btn2'), icon: Unlock, hidden: r.state !== 'banned', onSelect: () => unban(r.ip) },
  ]

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-48 flex-1 sm:max-w-sm">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
          <input type="search" className="input pl-9" placeholder={t('waf_filter_ip_ua')} aria-label={t('waf_filter_ip')} value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <span className="text-muted">{t('waf_sort')}</span>
          <select className="input w-auto py-1.5" value={sort} onChange={(e) => setSort(e.target.value)}>
            {SORTS.map(([v, k]) => (
              <option key={v} value={v}>
                {t(k)}
              </option>
            ))}
          </select>
        </label>
        <button type="button" className="btn btn-sm ml-auto" onClick={reload} aria-label={t('waf_refresh_ips')}>
          <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
        </button>
      </div>
      <ErrorBox error={error} onRetry={reload} />
      {loading && !data ? (
        <Spinner className="size-5" label={t('loading')} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>IP</th>
                <th>{t('waf_state')}</th>
                <SortHeader id="requests" label={t('requests')} sort={sort} onSort={setSort} className="text-right" />
                <SortHeader id="blocked" label={t('blocked_short')} sort={sort} onSort={setSort} className="text-right" />
                <th className="text-right">{t('errors_col')}</th>
                <SortHeader id="lastSeen" label={t('last_seen')} sort={sort} onSort={setSort} />
                <th>{t('waf_last_path')}</th>
                <th>
                  <span className="sr-only">{t('actions')}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.ip}>
                  <td className="whitespace-nowrap">
                    <span className="font-mono text-xs">{r.ip}</span>
                    {you && r.ip === you ? <span className="badge ml-2 border-brand/40 text-accent">{t('waf_you')}</span> : null}
                    {r.ua ? (
                      <p className="max-w-56 truncate text-[11px] text-muted" title={r.ua}>
                        {r.ua}
                      </p>
                    ) : null}
                  </td>
                  <td>
                    <IpStateBadge state={r.state} banExpires={r.banExpires} />
                  </td>
                  <td className="text-right tabular-nums">{formatNumber(r.requests)}</td>
                  <td className={`text-right tabular-nums ${r.blocked ? 'text-danger' : 'text-muted'}`}>{formatNumber(r.blocked)}</td>
                  <td className={`text-right tabular-nums ${r.errors ? 'text-warn' : 'text-muted'}`}>{formatNumber(r.errors)}</td>
                  <td className="text-xs whitespace-nowrap text-muted" title={t('waf_first_seen', { date: formatDate(r.firstSeen) })}>
                    {formatRelative(r.lastSeen)}
                  </td>
                  <td className="max-w-xs truncate font-mono text-xs" title={r.lastPath}>
                    {r.lastPath || '-'}
                  </td>
                  <td className="text-right">
                    <RowMenu label={t('waf_actions_for', { ip: r.ip })} items={menu(r)} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!rows.length ? <Empty>{filter ? t('logs_no_matches') : t('waf_no_clients')}</Empty> : null}
        </div>
      )}

      <RuleDialog
        open={dialog?.kind === 'blacklist' || dialog?.kind === 'whitelist'}
        list={dialog?.kind === 'whitelist' ? 'whitelist' : 'blacklist'}
        initialValue={dialog?.ip || ''}
        lockValue
        you={you}
        onClose={() => setDialog(null)}
        onSubmit={(payload) =>
          run(
            () => addWafRule(payload),
            payload.list === 'blacklist' ? t('waf_ip_blocked', { ip: payload.value }) : t('waf_ip_whitelisted', { ip: payload.value }),
            refreshAll,
          )
        }
      />
      <BanDialog
        open={dialog?.kind === 'ban'}
        ip={dialog?.ip}
        you={you}
        onClose={() => setDialog(null)}
        onSubmit={(payload) => run(() => banWafIp(payload), t('waf_banned_min', { ip: payload.ip, n: payload.minutes }), refreshAll)}
      />
    </div>
  )
}
