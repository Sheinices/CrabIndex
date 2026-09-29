// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { useState } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, Bot, Check, RefreshCw, ShieldBan, Undo2 } from 'lucide-react'
import { addWafBotRule, deleteWafBotRule, getWafBots, setWafBotCategory, setWafRobots } from '../../lib/api.js'
import { usePolling } from '../../hooks/usePolling.js'
import { useConfirm } from '../../components/Confirm.jsx'
import { ErrorBox, Spinner, Toggle } from '../../components/ui.jsx'
import { formatDate, formatNumber, formatRelative } from '../../lib/format.js'
import { BOT_STATUS, botCategoryWarning, botRuleOf } from '../../lib/waf.js'
import { Badge, Empty, RuleDialog, useWafAction } from './shared.jsx'
import { ListSection } from './WafRules.jsx'
import { useT } from '../../lang/index.jsx'

const arr = (v) => (Array.isArray(v) ? v : [])

function CategoryCard({ category, names, onToggle }) {
  const t = useT()
  const c = category
  const warning = botCategoryWarning(c.id)
  const headingId = `waf-bots-cat-${c.id}`
  return (
    <section className={`card flex min-w-0 flex-col gap-3 p-4 ${c.blockedCategory ? 'border-danger/40' : ''}`} aria-labelledby={headingId}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 id={headingId} className="font-semibold">
            {c.label}
          </h3>
          <p className="text-xs text-muted">{c.description}</p>
        </div>
        {c.blockedCategory ? <Badge tone="danger">{t('waf_bot_blocking')}</Badge> : null}
      </div>
      <dl className="grid grid-cols-3 gap-2 text-sm">
        <div>
          <dt className="text-xs text-muted">{t('requests')}</dt>
          <dd className="tabular-nums">{formatNumber(c.requests)}</dd>
        </div>
        <div>
          <dt className="text-xs text-muted">{t('blocked_short')}</dt>
          <dd className={`tabular-nums ${c.blocked ? 'text-danger' : 'text-muted'}`}>{formatNumber(c.blocked)}</dd>
        </div>
        <div>
          <dt className="text-xs text-muted">{t('waf_bots_count')}</dt>
          <dd className="tabular-nums">{formatNumber(c.botCount)}</dd>
        </div>
      </dl>
      {warning ? (
        <p className="flex gap-1.5 text-xs text-warn">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>{warning}</span>
        </p>
      ) : null}
      {names.length ? (
        <details className="text-xs">
          <summary className="cursor-pointer text-muted hover:text-fg">{t('waf_catalog_sigs')} · {names.length}</summary>
          <p className="mt-1 font-mono break-words text-muted">{names.join(', ')}</p>
        </details>
      ) : null}
      <div className="mt-auto pt-1">
        <Toggle id={`waf-bots-block-${c.id}`} checked={!!c.blockedCategory} onChange={(v) => onToggle(c, v)} label={t('waf_block_category')} />
      </div>
    </section>
  )
}

export function WafBots() {
  const t = useT()
  const confirm = useConfirm()
  const run = useWafAction()
  const [adding, setAdding] = useState(null)
  const { data, error, loading, reload } = usePolling(() => getWafBots(), 15_000)

  const d = data || {}
  const categories = arr(d.categories)
  const rows = arr(d.bots)
  const rules = d.rules || {}
  const catalog = d.catalog || {}
  const labelOf = Object.fromEntries(categories.map((c) => [c.id, c.label]))

  const toggleCategory = async (c, block) => {
    const warning = block ? botCategoryWarning(c.id) : null
    const ok = await confirm({
      title: block ? t('waf_block_cat_q', { label: c.label }) : t('waf_unblock_cat_q', { label: c.label }),
      message: (
        <>
          <p>{block ? t('waf_block_cat_msg') : t('waf_unblock_cat_msg')}</p>
          {warning ? <p className="font-medium text-warn">{warning}</p> : null}
        </>
      ),
      confirmLabel: block ? t('waf_block_btn') : t('waf_unblock_btn'),
      danger: block,
    })
    if (ok)
      await run(
        () => setWafBotCategory(c.id, block),
        block ? t('waf_cat_blocked', { label: c.label }) : t('waf_cat_unblocked', { label: c.label }),
        reload,
      )
  }

  const setRobots = (v) => run(() => setWafRobots(v), v ? t('waf_robots_on') : t('waf_robots_off'), reload)

  const quickRule = (list, name) =>
    run(() => addWafBotRule({ list, value: name }), list === 'botBlocked' ? t('waf_bot_blocked_t', { name }) : t('waf_bot_allowed_t', { name }), reload)

  const removeRule = async (list, value) => {
    const ok = await confirm({
      title: t('waf_remove_rule_q'),
      message: (
        <p>
          <span className="font-mono">{value}</span> {t('waf_delete_rule_msg', { list: t(`waf_list_${list}_t`) })}
        </p>
      ),
      confirmLabel: t('waf_remove_rule_btn'),
      danger: list === 'botAllowed',
    })
    if (ok) await run(() => deleteWafBotRule(list, value), t('waf_rule_removed', { value }), reload)
  }

  if (loading && !data) return <Spinner className="size-5" label={t('loading')} />

  return (
    <div className="space-y-6">
      <ErrorBox error={error} onRetry={reload} />
      <div role="note" className="card flex gap-3 p-4 text-sm">
        <Bot className="mt-0.5 size-5 shrink-0 text-muted" aria-hidden="true" />
        <div className="space-y-1">
          <p>{t('waf_bots_intro1')}</p>
          <p className="text-muted">{t('waf_bots_intro2')}</p>
        </div>
      </div>

      <section className="card flex flex-wrap items-center gap-3 p-4" aria-labelledby="waf-robots">
        <div className="min-w-0 flex-1">
          <h2 id="waf-robots" className="font-semibold">
            robots.txt
          </h2>
          <p className="text-xs text-muted">{rules.robotsDisallow ? t('waf_robots_on_desc') : t('waf_robots_off_desc')}</p>
        </div>
        <Toggle id="waf-robots-toggle" checked={!!rules.robotsDisallow} onChange={setRobots} label={t('waf_robots_toggle')} />
      </section>

      <section aria-labelledby="waf-bot-cats" className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h2 id="waf-bot-cats" className="font-semibold">
            {t('waf_categories')}
          </h2>
          <button type="button" className="btn btn-sm" onClick={reload} aria-label={t('refresh')}>
            <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
          </button>
        </div>
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {categories.map((c) => (
            <CategoryCard key={c.id} category={c} names={arr(catalog[c.id])} onToggle={toggleCategory} />
          ))}
        </div>
      </section>

      <section aria-labelledby="waf-bots-seen">
        <h2 id="waf-bots-seen" className="mb-3 font-semibold">
          {t('waf_bots_seen')} <span className="text-sm font-normal text-muted">· {rows.length}</span>
        </h2>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>{t('waf_bot')}</th>
                <th>{t('waf_category')}</th>
                <th className="text-right">{t('requests')}</th>
                <th className="text-right">{t('blocked_short')}</th>
                <th className="text-right">IP</th>
                <th>{t('last_seen')}</th>
                <th>{t('waf_paths')}</th>
                <th>{t('status')}</th>
                <th>
                  <span className="sr-only">{t('actions')}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((b) => {
                const rule = botRuleOf(b.name, rules)
                const status = BOT_STATUS[b.status] || BOT_STATUS.seen
                return (
                  <tr key={`${b.category}-${b.name}`}>
                    <td className="max-w-48">
                      <span className="block truncate font-mono text-xs" title={arr(b.samples).join('\n') || b.name}>
                        {b.name}
                      </span>
                    </td>
                    <td>
                      <Badge tone="brand">{labelOf[b.category] || b.category}</Badge>
                    </td>
                    <td className="text-right tabular-nums">{formatNumber(b.requests)}</td>
                    <td className={`text-right tabular-nums ${b.blocked ? 'text-danger' : 'text-muted'}`}>{formatNumber(b.blocked)}</td>
                    <td className="text-right tabular-nums">{formatNumber(b.ips)}</td>
                    <td className="text-xs whitespace-nowrap text-muted" title={formatDate(b.lastSeen)}>
                      {formatRelative(b.lastSeen)}
                    </td>
                    <td className="max-w-56">
                      {arr(b.topPaths)
                        .slice(0, 3)
                        .map((p) => (
                          <Link
                            key={p.path}
                            to={`/waf/log?path=${encodeURIComponent(p.path)}`}
                            className="block truncate font-mono text-xs text-accent hover:underline"
                            title={`${p.path} · ${p.requests}`}
                          >
                            {p.path}
                          </Link>
                        ))}
                    </td>
                    <td>
                      <Badge tone={status.tone}>{status.label}</Badge>
                    </td>
                    <td className="text-right whitespace-nowrap">
                      {rule ? (
                        <button type="button" className="btn btn-sm" onClick={() => removeRule(rule.list, rule.entry.value)} aria-label={t('waf_remove_rule_for', { name: b.name })}>
                          <Undo2 className="size-4" aria-hidden="true" /> {t('waf_remove_rule_btn')}
                        </button>
                      ) : (
                        <span className="inline-flex gap-1">
                          {b.status !== 'blocked' ? (
                            <button type="button" className="btn btn-sm btn-danger" onClick={() => quickRule('botBlocked', b.name)} aria-label={t('waf_block_name', { name: b.name })}>
                              <ShieldBan className="size-4" aria-hidden="true" /> {t('waf_block_btn')}
                            </button>
                          ) : null}
                          {b.status !== 'allowed' ? (
                            <button type="button" className="btn btn-sm" onClick={() => quickRule('botAllowed', b.name)} aria-label={t('waf_allow_name', { name: b.name })}>
                              <Check className="size-4" aria-hidden="true" /> {t('allow')}
                            </button>
                          ) : null}
                        </span>
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
          {!rows.length ? <Empty>{t('waf_no_bots')}</Empty> : null}
        </div>
      </section>

      <section aria-labelledby="waf-bot-rules" className="space-y-3">
        <div>
          <h2 id="waf-bot-rules" className="font-semibold">
            {t('waf_own_rules')}
          </h2>
          <p className="text-xs text-muted">{t('waf_own_rules_desc')}</p>
        </div>
        <div className="grid gap-6 lg:grid-cols-2">
          {['botBlocked', 'botAllowed'].map((list) => (
            <ListSection
              key={list}
              nested
              list={list}
              entries={arr(rules[list])}
              onAdd={() => setAdding(list)}
              onDelete={(e) => removeRule(list, e.value)}
            />
          ))}
        </div>
      </section>

      <RuleDialog
        open={!!adding}
        list={adding || 'botBlocked'}
        onClose={() => setAdding(null)}
        onSubmit={(payload) => run(() => addWafBotRule(payload), t('waf_added', { value: payload.value }), reload)}
      />
    </div>
  )
}
