import { useState } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, Globe, Lock, Plus, RefreshCw, RotateCcw, Settings, Trash2, Unlock } from 'lucide-react'
import { addWafRule, deleteWafRule, resetWafStats, unbanWafIp } from '../../lib/api.js'
import { useConfirm } from '../../components/Confirm.jsx'
import { ErrorBox, Spinner, StatusDot } from '../../components/ui.jsx'
import { formatDate, formatDuration, formatRelative } from '../../lib/format.js'
import { Empty, ReasonBadge, RuleDialog, SETTINGS_WAF, useWafAction } from './shared.jsx'
import { useWaf } from './Waf.jsx'

const LISTS = {
  blacklist: { title: 'Чёрный список', hint: 'Запросы получают 403 Forbidden', add: 'Заблокировать', danger: true },
  whitelist: { title: 'Белый список', hint: 'Без лимитов, ловушек и банов', add: 'Добавить' },
  domainBlacklist: { title: 'Чёрный список доменов', hint: 'Origin/Referer домена и поддоменов → 403, без бана', add: 'Заблокировать', danger: true },
  domainWhitelist: { title: 'Белый список доменов', hint: 'Без лимита, ловушек и фильтра User-Agent', add: 'Разрешить' },
  botBlocked: { title: 'Заблокированные боты', hint: 'User-Agent содержит значение (или это имя бота) → 403, без бана', add: 'Заблокировать', danger: true },
  botAllowed: { title: 'Разрешённые боты', hint: 'Не блокируются правилами ботов, даже если заблокирована категория', add: 'Разрешить' },
}

export function expiresText(value) {
  if (!value) return 'бессрочно'
  const left = Math.round((new Date(value).getTime() - Date.now()) / 1000)
  if (!Number.isFinite(left)) return String(value)
  return left > 0 ? `ещё ${formatDuration(left)}` : 'истекло'
}

export function ListSection({ list, entries, onAdd, onDelete, nested = false }) {
  const meta = LISTS[list]
  const Heading = nested ? 'h3' : 'h2'
  return (
    <section className="card min-w-0 p-5" aria-labelledby={`waf-${list}`}>
      <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
        <div>
          <Heading id={`waf-${list}`} className="font-semibold">
            {meta.title} <span className="text-sm font-normal text-muted">· {entries.length}</span>
          </Heading>
          <p className="text-xs text-muted">{meta.hint}</p>
        </div>
        <button type="button" className={`btn btn-sm ${meta.danger ? 'btn-danger' : 'btn-primary'}`} onClick={onAdd}>
          <Plus className="size-4" aria-hidden="true" /> {meta.add}
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
                  <span title={formatDate(e.created)}>добавлено {formatRelative(e.created)}</span> ·{' '}
                  <span title={e.expires ? formatDate(e.expires) : undefined}>{expiresText(e.expires)}</span>
                </p>
              </div>
              <button type="button" className="btn btn-ghost btn-sm text-danger" onClick={() => onDelete(e)} aria-label={`Удалить ${e.value} из списка «${meta.title}»`}>
                <Trash2 className="size-4" aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <Empty>Список пуст</Empty>
      )}
    </section>
  )
}

function BuiltinDomains({ domains }) {
  return (
    <section className="card min-w-0 p-5" aria-labelledby="waf-builtin-domains">
      <div className="mb-3 flex items-start gap-2">
        <Lock className="mt-0.5 size-4 shrink-0 text-muted" aria-hidden="true" />
        <div>
          <h3 id="waf-builtin-domains" className="font-semibold">
            Встроенные заблокированные домены <span className="text-sm font-normal text-muted">· {domains.length}</span>
          </h3>
          <p className="text-xs text-muted">
            Встроенный список, изменить нельзя: он зашит в программу. Запросы с этих доменов и их поддоменов получают 403 всегда - даже с IP из белого списка и из
            LAN (кроме localhost).
          </p>
        </div>
      </div>
      {domains.length ? (
        <ul className="flex flex-wrap gap-1.5" aria-label="Встроенный список доменов">
          {domains.map((d) => (
            <li key={d} className="inline-flex items-center gap-1 rounded bg-surface-2 px-1.5 py-0.5 font-mono text-xs" title="Встроенный список, изменить нельзя">
              <Lock className="size-3 text-muted" aria-hidden="true" />
              {d}
            </li>
          ))}
        </ul>
      ) : (
        <Empty>Список пуст</Empty>
      )}
    </section>
  )
}

function AllowlistOnlyState({ value }) {
  return (
    <div className="flex flex-wrap items-center gap-3 text-sm">
      <span className="text-muted">
        Только разрешённые домены <span className="font-mono text-[11px] opacity-70">waf.domainAllowlistOnly</span>
      </span>
      <StatusDot tone={value ? 'warn' : 'muted'} label={value ? 'включено' : 'выключено'} />
      <span className="text-xs text-muted">
        {value
          ? 'Запросы с Origin/Referer не из белого списка доменов и не с этого сервера получают 403.'
          : 'Запросы с любых доменов, кроме заблокированных, пропускаются.'}
      </span>
      <Link to={SETTINGS_WAF} className="btn btn-sm ml-auto">
        <Settings className="size-4" aria-hidden="true" /> Изменить в настройках
      </Link>
    </div>
  )
}

const CONFIG_ROWS = [
  ['enable', 'Включён'],
  ['logRequests', 'Журнал запросов'],
  ['historySize', 'Запросов в памяти'],
  ['rateLimit.enable', 'Лимит запросов'],
  ['rateLimit.perMinute', 'Запросов на IP в минуту'],
  ['rateLimit.banMinutes', 'Бан за превышение, мин'],
  ['trapPaths', 'Пути-ловушки'],
  ['trapBanMinutes', 'Бан за ловушку, мин'],
  ['blockUserAgents', 'Блокируемые User-Agent'],
  ['whitelistLan', 'LAN без ограничений'],
  ['domainAllowlistOnly', 'Только разрешённые домены'],
]

function ConfigValue({ value }) {
  if (typeof value === 'boolean') return <StatusDot tone={value ? 'ok' : 'muted'} label={value ? 'да' : 'нет'} />
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
      title: 'Удалить правило?',
      message: (
        <p>
          <span className="font-mono">{entry.value}</span> будет удалён из списка «{LISTS[list].title}».
        </p>
      ),
      confirmLabel: 'Удалить',
      danger: true,
    })
    if (ok) await run(() => deleteWafRule(list, entry.value), `Удалено: ${entry.value}`, rules.reload)
  }

  const unban = async (ban) => {
    const ok = await confirm({ title: 'Снять бан?', message: <p>IP <span className="font-mono">{ban.ip}</span> будет разбанен.</p>, confirmLabel: 'Снять бан' })
    if (ok) await run(() => unbanWafIp(ban.ip), `Бан снят: ${ban.ip}`, rules.reload)
  }

  const reset = async () => {
    const ok = await confirm({
      title: 'Сбросить статистику?',
      message: <p>Журнал запросов, счётчики и график будут очищены. Списки и баны сохранятся.</p>,
      confirmLabel: 'Сбросить',
      danger: true,
    })
    if (ok) await run(() => resetWafStats(), 'Статистика WAF сброшена')
  }

  if (rules.loading && !rules.data) return <Spinner className="size-5" label="Загрузка…" />

  return (
    <div className="space-y-6">
      <ErrorBox error={rules.error} onRetry={rules.reload} />
      <div role="note" className="card flex gap-3 border-warn/40 bg-warn/10 p-4 text-sm">
        <AlertTriangle className="mt-0.5 size-5 shrink-0 text-warn" aria-hidden="true" />
        <div>
          <p className="font-medium">
            Ваш IP: <span className="font-mono">{you || 'неизвестен'}</span>
          </p>
          <p className="text-muted">
            Заблокировать или забанить собственный IP (или подсеть, в которую он входит) и loopback-адреса нельзя - сервер отклонит такое правило, чтобы вы не потеряли доступ к
            панели.
          </p>
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
            <Globe className="size-4 text-muted" aria-hidden="true" /> Домены
          </h2>
          <p className="text-xs text-muted">
            Домен запроса - хост заголовка Origin, а если его нет - Referer. Правило example.com действует и на все поддомены. Запросы без Origin и Referer по домену не
            блокируются; за блокировку по домену IP не банится.
          </p>
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
            Активные баны <span className="text-sm font-normal text-muted">· {bans.length}</span>
          </h2>
          <button type="button" className="btn btn-sm" onClick={rules.reload} aria-label="Обновить правила">
            <RefreshCw className={`size-4 ${rules.loading ? 'animate-spin' : ''}`} aria-hidden="true" />
          </button>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>IP</th>
                <th>Причина</th>
                <th>Начало</th>
                <th>До</th>
                <th>
                  <span className="sr-only">Действия</span>
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
                    {formatDate(b.expires)} <span className="text-muted">({expiresText(b.expires)})</span>
                  </td>
                  <td className="text-right">
                    <button type="button" className="btn btn-sm" onClick={() => unban(b)}>
                      <Unlock className="size-4" aria-hidden="true" /> Снять бан
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!bans.length ? <Empty>Активных банов нет</Empty> : null}
        </div>
      </section>

      <div className="grid gap-6 lg:grid-cols-3">
        <section className="card p-5 lg:col-span-2" aria-labelledby="waf-config">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h2 id="waf-config" className="font-semibold">
              Конфигурация <span className="text-sm font-normal text-muted">· только чтение</span>
            </h2>
            <Link to={SETTINGS_WAF} className="btn btn-sm">
              <Settings className="size-4" aria-hidden="true" /> Настройки → WAF
            </Link>
          </div>
          <dl className="divide-y divide-border text-sm">
            {CONFIG_ROWS.map(([key, label]) => (
              <div key={key} className="flex items-start justify-between gap-4 py-2">
                <dt className="text-muted">
                  {label} <span className="font-mono text-[11px] opacity-70">waf.{key}</span>
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
            Статистика
          </h2>
          <p className="mb-4 text-sm text-muted">Счётчики и журнал хранятся в памяти и обнуляются при перезапуске. Списки и баны лежат в Data/waf.json.</p>
          <button type="button" className="btn btn-danger" onClick={reset}>
            <RotateCcw className="size-4" aria-hidden="true" /> Сбросить статистику
          </button>
        </section>
      </div>

      <RuleDialog
        open={!!adding}
        list={adding || 'blacklist'}
        you={you}
        builtinDomains={builtinDomains}
        onClose={() => setAdding(null)}
        onSubmit={(payload) => run(() => addWafRule(payload), `Добавлено: ${payload.value}`, rules.reload)}
      />
    </div>
  )
}
