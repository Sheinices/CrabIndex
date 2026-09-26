import { useCallback, useEffect, useId, useState } from 'react'
import { Link } from 'react-router'
import { ShieldOff } from 'lucide-react'
import { Modal } from '../../components/Modal.jsx'
import { useToast } from '../../components/Toast.jsx'
import {
  BAN_PRESETS,
  IP_STATES,
  TONE_BADGE,
  reasonLabel,
  selfBlockError,
  statusTone,
  validateRuleValue,
  BLOCK_REASONS,
  domainRuleError,
  isDomainList,
  isBotList,
  normalizeDomain,
  validateBotRule,
} from '../../lib/waf.js'
import { formatDate } from '../../lib/format.js'
import { useT } from '../../lang/index.jsx'

export const SETTINGS_WAF = '/settings?group=waf'

export function Badge({ tone = 'muted', children, title }) {
  return (
    <span className={`badge ${TONE_BADGE[tone] || ''}`} title={title}>
      {children}
    </span>
  )
}

export function StatusBadge({ status }) {
  return <Badge tone={statusTone(status)}>{status ?? '-'}</Badge>
}

export function ReasonBadge({ reason }) {
  if (!reason) return null
  return <Badge tone={BLOCK_REASONS[reason]?.tone || 'danger'}>{reasonLabel(reason)}</Badge>
}

export function IpStateBadge({ state, banExpires }) {
  const t = useT()
  const meta = IP_STATES[state] || IP_STATES.normal
  const text = state === 'banned' && banExpires ? t('waf_banned_until', { date: formatDate(banExpires) }) : meta.label
  return <Badge tone={meta.tone}>{text}</Badge>
}

export function DisabledNotice() {
  const t = useT()
  return (
    <div role="status" className="card mb-6 flex flex-wrap items-center gap-3 border-warn/40 bg-warn/10 p-4 text-sm">
      <ShieldOff className="size-5 shrink-0 text-warn" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="font-medium">{t('waf_disabled')}</p>
        <p className="text-muted">{t('waf_disabled_desc')}</p>
      </div>
      <Link to={SETTINGS_WAF} className="btn btn-sm">
        {t('waf_open_settings')}
      </Link>
    </div>
  )
}

/**
 * Run a WAF mutation with toasts: success message on `{ok:true}`, server error
 * text (`{ok:false,error}` or HTTP error) otherwise. Returns true on success.
 */
export function useWafAction() {
  const t = useT()
  const toast = useToast()
  return useCallback(
    async (fn, success, after) => {
      try {
        await fn()
        if (success) toast.success(success)
        await after?.()
        return true
      } catch (e) {
        if (e?.status !== 401) toast.error(t('waf_err_title'), e?.message || String(e))
        return false
      }
    },
    [toast, t],
  )
}

// i18n keys; rendered with t() inside RuleDialog.
export const EXPIRY_OPTIONS = [
  { value: '', labelKey: 'waf_exp_forever' },
  { value: '60', labelKey: 'waf_exp_1h' },
  { value: '1440', labelKey: 'waf_exp_24h' },
  { value: '10080', labelKey: 'waf_exp_7d' },
  { value: '43200', labelKey: 'waf_exp_30d' },
  { value: 'custom', labelKey: 'waf_exp_custom' },
]

/** Parse the minutes input; returns a positive integer or null. */
export function parseMinutes(v) {
  const n = Number(String(v).trim())
  return Number.isInteger(n) && n > 0 ? n : null
}

// i18n key pairs per list: `waf_dlg_<list>_t` (title) / `waf_dlg_<list>_d` (description).
const DIALOG_LISTS = ['blacklist', 'whitelist', 'domainBlacklist', 'domainWhitelist', 'botBlocked', 'botAllowed']

/**
 * Add-to-list form (IP, domain or bot blacklist / whitelist). `onSubmit({list, value,
 * comment, expiresMinutes})` must resolve to true on success (closes the dialog).
 * `builtinDomains` is used to reject builtin conflicts before the request.
 */
export function RuleDialog({ open, onClose, list, initialValue = '', you, onSubmit, lockValue = false, builtinDomains = [] }) {
  const t = useT()
  const ids = useId()
  const [value, setValue] = useState(initialValue)
  const [comment, setComment] = useState('')
  const [expiry, setExpiry] = useState('')
  const [custom, setCustom] = useState('')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (open) {
      setValue(initialValue)
      setComment('')
      setExpiry('')
      setCustom('')
      setError(null)
    }
  }, [open, initialValue])

  const domain = isDomainList(list)
  const bot = isBotList(list)
  const black = list === 'blacklist' || list === 'domainBlacklist' || list === 'botBlocked'
  const dlg = DIALOG_LISTS.includes(list) ? list : 'blacklist'
  const submit = async (e) => {
    e.preventDefault()
    const v = domain ? normalizeDomain(value) : value.trim()
    const err = bot
      ? validateBotRule(v)
      : domain
        ? domainRuleError(list, value, builtinDomains)
        : validateRuleValue(v) || (black ? selfBlockError(v, you) : null)
    if (err) return setError(err)
    let expiresMinutes
    if (expiry === 'custom') {
      expiresMinutes = parseMinutes(custom)
      if (!expiresMinutes) return setError(t('waf_custom_minutes_err'))
    } else if (expiry) expiresMinutes = Number(expiry)
    setError(null)
    setBusy(true)
    const payload = { list, value: v }
    if (comment.trim()) payload.comment = comment.trim()
    if (expiresMinutes) payload.expiresMinutes = expiresMinutes
    const ok = await onSubmit(payload)
    setBusy(false)
    if (ok) onClose()
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t(`waf_dlg_${dlg}_t`)}
      description={t(`waf_dlg_${dlg}_d`)}
      size="sm"
    >
      <form id={`${ids}-form`} onSubmit={submit} className="space-y-4" noValidate>
        <div>
          <label className="label" htmlFor={`${ids}-v`}>
            {bot ? t('waf_label_bot') : domain ? t('domain') : t('waf_label_ip')}
          </label>
          <input
            id={`${ids}-v`}
            className="input font-mono"
            value={value}
            readOnly={lockValue}
            onChange={(e) => setValue(e.target.value)}
            placeholder={bot ? t('waf_ph_bot') : domain ? 'example.com' : t('waf_ph_ip')}
            aria-invalid={!!error}
            aria-describedby={error ? `${ids}-err` : undefined}
            data-autofocus={lockValue ? undefined : true}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div>
          <label className="label" htmlFor={`${ids}-c`}>
            {t('waf_comment_opt')}
          </label>
          <input id={`${ids}-c`} className="input" value={comment} onChange={(e) => setComment(e.target.value)} maxLength={200} />
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label className="label" htmlFor={`${ids}-e`}>
              {t('waf_expiry')}
            </label>
            <select id={`${ids}-e`} className="input" value={expiry} onChange={(e) => setExpiry(e.target.value)}>
              {EXPIRY_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {t(o.labelKey)}
                </option>
              ))}
            </select>
          </div>
          {expiry === 'custom' ? (
            <div>
              <label className="label" htmlFor={`${ids}-m`}>
                {t('waf_minutes')}
              </label>
              <input id={`${ids}-m`} className="input" inputMode="numeric" value={custom} onChange={(e) => setCustom(e.target.value)} placeholder="90" />
            </div>
          ) : null}
        </div>
        {domain ? <p className="text-xs text-muted">{t('waf_domain_hint')}</p> : null}
        {bot ? <p className="text-xs text-muted">{t('waf_bot_hint')}</p> : null}
        {error ? (
          <p id={`${ids}-err`} role="alert" className="text-sm text-danger">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2 pt-1">
          <button type="button" className="btn" onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="submit" className={`btn ${black ? 'btn-danger' : 'btn-primary'}`} disabled={busy}>
            {black ? t('block') : t('add')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

/** Temporary ban with a custom duration. */
export function BanDialog({ open, onClose, ip, you, onSubmit }) {
  const t = useT()
  const ids = useId()
  const [minutes, setMinutes] = useState('120')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (open) {
      setMinutes('120')
      setError(null)
    }
  }, [open])

  const submit = async (e) => {
    e.preventDefault()
    const m = parseMinutes(minutes)
    if (!m) return setError(t('waf_ban_minutes_err'))
    const self = selfBlockError(ip, you)
    if (self) return setError(self)
    setBusy(true)
    const ok = await onSubmit({ ip, minutes: m, reason: 'manual' })
    setBusy(false)
    if (ok) onClose()
  }

  return (
    <Modal open={open} onClose={onClose} title={t('waf_ban_custom_title')} description={ip} size="sm">
      <form onSubmit={submit} className="space-y-4" noValidate>
        <div>
          <label className="label" htmlFor={`${ids}-m`}>
            {t('waf_ban_duration')}
          </label>
          <input id={`${ids}-m`} className="input" inputMode="numeric" value={minutes} onChange={(e) => setMinutes(e.target.value)} data-autofocus />
          <div className="mt-2 flex flex-wrap gap-1.5">
            {[...BAN_PRESETS, { minutes: 10080, label: t('waf_7d') }].map((p) => (
              <button key={p.minutes} type="button" className="btn btn-sm" onClick={() => setMinutes(String(p.minutes))}>
                {p.label}
              </button>
            ))}
          </div>
        </div>
        {error ? (
          <p role="alert" className="text-sm text-danger">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2">
          <button type="button" className="btn" onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="submit" className="btn btn-danger" disabled={busy}>
            {t('waf_ban_btn')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

export function Empty({ children }) {
  return <p className="px-3 py-6 text-center text-sm text-muted">{children}</p>
}
