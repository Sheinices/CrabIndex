import { useState } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, Globe, Lock, Plus, RefreshCw, RotateCcw, Settings, Trash2, Unlock } from 'lucide-react'
import { addWafRule, deleteWafRule, resetWafStats, unbanWafIp } from '../../lib/api.js'
import { useConfirm } from '../../components/Confirm.jsx'
import { ErrorBox, Spinner, StatusDot } from '../../components/ui.jsx'
import { formatDate, formatDuration, formatRelative } from '../../lib/format.js'
import { Empty, ReasonBadge, RuleDialog, SETTINGS_WAF, useWafAction } from './shared.jsx'
import { useWaf } from './Waf.jsx'
import { tBase, useT } from '../../lang/index.jsx'

// Per-list i18n keys: `waf_list_<list>_t` (title) / `waf_list_<list>_h` (hint); `add` is a common key.
export const LISTS = {
  blacklist: { add: 'block', danger: true },
  whitelist: { add: 'add' },
  domainBlacklist: { add: 'block', danger: true },
  domainWhitelist: { add: 'allow' },
  botBlocked: { add: 'block', danger: true },
  botAllowed: { add: 'allow' },
}

/** Human "expires" text; pass the component's `t` for the active language (defaults to the Russian base). */
export function expiresText(value, t = tBase) {
  if (!value) return t('waf_exp_none')
  const left = Math.round((new Date(value).getTime() - Date.now()) / 1000)
  if (!Number.isFinite(left)) return String(value)
  return left > 0 ? t('waf_exp_left', { d: formatDuration(left) }) : t('waf_exp_expired')
}

export function ListSection({ list, entries, onAdd, onDelete, nested = false }) {
  const t = useT()
  const meta = LISTS[list]
  const title = t(`waf_list_${list}_t`)
  const Heading = nested ? 'h3' : 'h2'
  return (
    <section className="card min-w-0 p-5" aria-labelledby={`waf-${list}`}>
      <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
        <div>
          <Heading id={`waf-${list}`} className="font-semibold">
            {title} <span className="text-sm font-normal text-muted">· {entries.length}</span>
          </Heading>
          <p className="text-xs text-muted">{t(`waf_list_${list}_h`)}</p>
        </div>
        <button type="button" className={`btn btn-sm ${meta.danger ? 'btn-danger' : 'btn-primary'}`} onClick={onAdd}>
          <Plus className="size-4" aria-hidden="true" /> {t(meta.add)}
        </button>
      </div>
      {entries.length ? (
        <ul className="divide-y divide-border">
          {entries.map((e) => (
            <li key={e.value} className="flex items-start gap-3 py-2">
              <div className="min-w-0 flex-1">
                <p className="font-mono text-sm break-all">{e.value}</p>
                <p className="text-xs text-muted">
                  {e.comment ? <span className="text-fg">{e.comment} · </span> : null}
                  <span title={formatDate(e.created)}>{t('waf_added_ago', { rel: formatRelative(e.created) })}</span> ·{' '}
                  <span title={e.expires ? formatDate(e.expires) : undefined}>{expiresText(e.expires, t)}</span>
                </p>
              </div>
              <button type="button" className="btn btn-ghost btn-sm text-danger" onClick={() => onDelete(e)} aria-label={t('waf_delete_from', { value: e.value, list: title })}>
                <Trash2 className="size-4" aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <Empty>{t('waf_list_empty')}</Empty>
      )}
    </section>
  )
}

function BuiltinDomains({ domains }) {
  const t = useT()
  return (
    <section className="card min-w-0 p-5" aria-labelledby="waf-builtin-domains">
      <div className="mb-3 flex items-start gap-2">
        <Lock className="mt-0.5 size-4 shrink-0 text-muted" aria-hidden="true" />
        <div>
          <h3 id="waf-builtin-domains" className="font-semibold">
            {t('waf_builtin_title')} <span className="text-sm font-normal text-muted">· {domains.length}</span>
          </h3>
          <p className="text-xs text-muted">{t('waf_builtin_desc')}</p>
        </div>
      </div>
      {domains.length ? (
        <ul className="flex flex-wrap gap-1.5" aria-label={t('waf_builtin_list')}>
          {domains.map((d) => (
            <li key={d} className="inline-flex items-center gap-1 rounded bg-surface-2 px-1.5 py-0.5 font-mono text-xs" title={t('waf_builtin_locked')}>
              <Lock className="size-3 text-muted" aria-hidden="true" />
              {d}
            </li>
          ))}
        </ul>
      ) : (
        <Empty>{t('waf_list_empty')}</Empty>
      )}
    </section>
  )
}

function AllowlistOnlyState({ value }) {
  const t = useT()
  return (
    <div className="flex flex-wrap items-center gap-3 text-sm">
      <span className="text-muted">
        {t('waf_allowlist_only')} <span className="font-mono text-[11px] opacity-70">waf.domainAllowlistOnly</span>
      </span>
      <StatusDot tone={value ? 'warn' : 'muted'} label={value ? t('waf_on') : t('waf_off_state')} />
      <span className="text-xs text-muted">{value ? t('waf_allowlist_on_desc') : t('waf_allowlist_off_desc')}</span>
      <Link to={SETTINGS_WAF} className="btn btn-sm ml-auto">
        <Settings className="size-4" aria-hidden="true" /> {t('waf_change_in_settings')}
      </Link>
    </div>
  )
}

// [config path, i18n key]
const CONFIG_ROWS = [
  ['enable', 'waf_cfg_enable'],
  ['logRequests', 'waf_cfg_logRequests'],
  ['historySize', 'waf_cfg_historySize'],
  ['rateLimit.enable', 'waf_cfg_rateLimit'],
  ['rateLimit.perMinute', 'waf_cfg_perMinute'],
  ['rateLimit.banMinutes', 'waf_cfg_banMinutes'],
  ['trapPaths', 'waf_cfg_trapPaths'],
  ['trapBanMinutes', 'waf_cfg_trapBan'],
  ['blockUserAgents', 'waf_cfg_blockUA'],
  ['whitelistLan', 'waf_cfg_lan'],
  ['domainAllowlistOnly', 'waf_allowlist_only'],
]

function ConfigValue({ value }) {
  const t = useT()
  if (typeof value === 'boolean') return <StatusDot tone={value ? 'ok' : 'muted'} label={value ? t('yes') : t('no')} />
  if (Array.isArray(value)) {
    if (!value.length) return <span className="text-muted">-</span>
    return (
      <span className="flex flex-wrap justify-end gap-1">
        {value.map((v) => (
          <code key={String(v)} className="rounded bg-surface-2 px-1.5 py-0.5 text-xs">
            {String(v)}
          </code>
        ))}
      </span>
    )
  }
  if (value == null || value === '') return <span className="text-muted">-</span>
  return <span className="tabular-nums">{String(value)}</span>
}

const pick = (obj, path) => path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj)

export function WafRules() {
  const t = useT()
  const { rules, you } = useWaf()
  const confirm = useConfirm()
  const run = useWafAction()
  const [adding, setAdding] = useState(null)
  const data = rules.data || {}
  const bans = Array.isArray(data.bans) ? data.bans : []
  const builtinDomains = Array.isArray(data.builtinDomains) ? data.builtinDomains : []
  const entries = (list) => (Array.isArray(data[list]) ? data[list] : [])

  const remove = async (list, entry) => {
    const ok = await confirm({
      title: t('waf_delete_rule_q'),
      message: (
        <p>
          <span className="font-mono">{entry.value}</span> {t('waf_delete_rule_msg', { list: t(`waf_list_${list}_t`) })}
        </p>
      ),
      confirmLabel: t('delete'),
      danger: true,
    })
    if (ok) await run(() => deleteWafRule(list, entry.value), t('waf_deleted', { value: entry.value }), rules.reload)
  }

  const unban = async (ban) => {
    const ok = await confirm({
      title: t('waf_unban_q'),
      message: (
        <p>
          IP <span className="font-mono">{ban.ip}</span> {t('waf_unban_msg_rules')}
        </p>
      ),
      confirmLabel: t('waf_unban_btn'),
    })
    if (ok) await run(() => unbanWafIp(ban.ip), t('waf_ban_lifted', { ip: ban.ip }), rules.reload)
  }

  const reset = async () => {
    const ok = await confirm({
      title: t('waf_reset_q'),
      message: <p>{t('waf_reset_msg')}</p>,
      confirmLabel: t('reset'),
      danger: true,
    })
    if (ok) await run(() => resetWafStats(), t('waf_reset_done'))
  }

  if (rules.loading && !rules.data) return <Spinner className="size-5" label={t('loading')} />

  return (
    <div className="space-y-6">
      <ErrorBox error={rules.error} onRetry={rules.reload} />
      <div role="note" className="card flex gap-3 border-warn/40 bg-warn/10 p-4 text-sm">
        <AlertTriangle className="mt-0.5 size-5 shrink-0 text-warn" aria-hidden="true" />
        <div>
          <p className="font-medium">
            {t('waf_your_ip')} <span className="font-mono">{you || t('waf_unknown')}</span>
          </p>
          <p className="text-muted">{t('waf_self_note')}</p>
        </div>
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        {['blacklist', 'whitelist'].map((list) => (
          <ListSection
            key={list}
            list={list}
            entries={entries(list)}
            onAdd={() => setAdding(list)}
            onDelete={(e) => remove(list, e)}
          />
        ))}
      </div>

      <section aria-labelledby="waf-domains" className="space-y-4">
        <div>
          <h2 id="waf-domains" className="flex items-center gap-2 font-semibold">
            <Globe className="size-4 text-muted" aria-hidden="true" /> {t('waf_domains')}
          </h2>
          <p className="text-xs text-muted">{t('waf_domains_desc')}</p>
        </div>
        <div className="card p-4">
          <AllowlistOnlyState value={!!data.config?.domainAllowlistOnly} />
        </div>
        <BuiltinDomains domains={builtinDomains} />
        <div className="grid gap-6 lg:grid-cols-2">
          {['domainBlacklist', 'domainWhitelist'].map((list) => (
            <ListSection key={list} nested list={list} entries={entries(list)} onAdd={() => setAdding(list)} onDelete={(e) => remove(list, e)} />
          ))}
        </div>
      </section>

      <section aria-labelledby="waf-bans">
        <div className="mb-3 flex items-center justify-between gap-2">
          <h2 id="waf-bans" className="font-semibold">
            {t('waf_active_bans')} <span className="text-sm font-normal text-muted">· {bans.length}</span>
          </h2>
          <button type="button" className="btn btn-sm" onClick={rules.reload} aria-label={t('waf_refresh_rules')}>
            <RefreshCw className={`size-4 ${rules.loading ? 'animate-spin' : ''}`} aria-hidden="true" />
          </button>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>IP</th>
                <th>{t('waf_reason')}</th>
                <th>{t('waf_start')}</th>
                <th>{t('waf_until')}</th>
                <th>
                  <span className="sr-only">{t('actions')}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {bans.map((b) => (
                <tr key={b.ip}>
                  <td>
                    <Link to={`/waf/log?ip=${encodeURIComponent(b.ip)}`} className="font-mono text-xs text-accent hover:underline">
                      {b.ip}
                    </Link>
                  </td>
                  <td>
                    <ReasonBadge reason={b.reason} />
                  </td>
                  <td className="text-xs whitespace-nowrap text-muted">{formatDate(b.created)}</td>
                  <td className="text-xs whitespace-nowrap">
                    {formatDate(b.expires)} <span className="text-muted">({expiresText(b.expires, t)})</span>
                  </td>
                  <td className="text-right">
                    <button type="button" className="btn btn-sm" onClick={() => unban(b)}>
                      <Unlock className="size-4" aria-hidden="true" /> {t('waf_unban_btn')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!bans.length ? <Empty>{t('waf_no_bans')}</Empty> : null}
        </div>
      </section>

      <div className="grid gap-6 lg:grid-cols-3">
        <section className="card p-5 lg:col-span-2" aria-labelledby="waf-config">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h2 id="waf-config" className="font-semibold">
              {t('waf_config')} <span className="text-sm font-normal text-muted">{t('waf_readonly')}</span>
            </h2>
            <Link to={SETTINGS_WAF} className="btn btn-sm">
              <Settings className="size-4" aria-hidden="true" /> {t('waf_settings_arrow')}
            </Link>
          </div>
          <dl className="divide-y divide-border text-sm">
            {CONFIG_ROWS.map(([key, labelKey]) => (
              <div key={key} className="flex items-start justify-between gap-4 py-2">
                <dt className="text-muted">
                  {t(labelKey)} <span className="font-mono text-[11px] opacity-70">waf.{key}</span>
                </dt>
                <dd className="text-right">
                  <ConfigValue value={pick(data.config, key)} />
                </dd>
              </div>
            ))}
          </dl>
        </section>
        <section className="card p-5" aria-labelledby="waf-reset">
          <h2 id="waf-reset" className="mb-2 font-semibold">
            {t('waf_stats')}
          </h2>
          <p className="mb-4 text-sm text-muted">{t('waf_stats_note')}</p>
          <button type="button" className="btn btn-danger" onClick={reset}>
            <RotateCcw className="size-4" aria-hidden="true" /> {t('cf_reset_stats')}
          </button>
        </section>
      </div>

      <RuleDialog
        open={!!adding}
        list={adding || 'blacklist'}
        you={you}
        builtinDomains={builtinDomains}
        onClose={() => setAdding(null)}
        onSubmit={(payload) => run(() => addWafRule(payload), t('waf_added', { value: payload.value }), rules.reload)}
      />
    </div>
  )
}
