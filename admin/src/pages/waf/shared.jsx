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
  const meta = IP_STATES[state] || IP_STATES.normal
  const text = state === 'banned' && banExpires ? `забанен до ${formatDate(banExpires)}` : meta.label
  return <Badge tone={meta.tone}>{text}</Badge>
}

export function DisabledNotice() {
  return (
    <div role="status" className="card mb-6 flex flex-wrap items-center gap-3 border-warn/40 bg-warn/10 p-4 text-sm">
      <ShieldOff className="size-5 shrink-0 text-warn" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <p className="font-medium">WAF выключен</p>
        <p className="text-muted">Запросы не фильтруются и статистика не собирается. Включите его в настройках (waf.enable).</p>
      </div>
      <Link to={SETTINGS_WAF} className="btn btn-sm">
        Открыть настройки WAF
      </Link>
    </div>
  )
}

/**
 * Run a WAF mutation with toasts: success message on `{ok:true}`, server error
 * text (`{ok:false,error}` or HTTP error) otherwise. Returns true on success.
 */
export function useWafAction() {
  const toast = useToast()
  return useCallback(
    async (fn, success, after) => {
      try {
        await fn()
        if (success) toast.success(success)
        await after?.()
        return true
      } catch (e) {
        if (e?.status !== 401) toast.error('Ошибка WAF', e?.message || String(e))
        return false
      }
    },
    [toast],
  )
}

export const EXPIRY_OPTIONS = [
  { value: '', label: 'Бессрочно' },
  { value: '60', label: '1 час' },
  { value: '1440', label: '24 часа' },
  { value: '10080', label: '7 дней' },
  { value: '43200', label: '30 дней' },
  { value: 'custom', label: 'Свой срок…' },
]

/** Parse the minutes input; returns a positive integer or null. */
export function parseMinutes(v) {
  const n = Number(String(v).trim())
  return Number.isInteger(n) && n > 0 ? n : null
}

const DIALOG_TEXT = {
  blacklist: { title: 'Добавить в чёрный список', description: 'Запросы с адреса будут получать 403.' },
  whitelist: { title: 'Добавить в белый список', description: 'Адрес не будет ограничиваться и баниться.' },
  domainBlacklist: { title: 'Заблокировать домен', description: 'Запросы с Origin/Referer этого домена и его поддоменов будут получать 403.' },
  domainWhitelist: {
    title: 'Разрешить домен',
    description: 'Запросы с этого домена и его поддоменов не проверяются лимитом, ловушками и фильтром User-Agent.',
  },
  botBlocked: { title: 'Заблокировать бота', description: 'Запросы с таким User-Agent будут получать 403. IP не банится.' },
  botAllowed: { title: 'Разрешить бота', description: 'Запросы с таким User-Agent не блокируются правилами ботов, даже если заблокирована вся категория.' },
}

/**
 * Add-to-list form (IP, domain or bot blacklist / whitelist). `onSubmit({list, value,
 * comment, expiresMinutes})` must resolve to true on success (closes the dialog).
 * `builtinDomains` is used to reject builtin conflicts before the request.
 */
export function RuleDialog({ open, onClose, list, initialValue = '', you, onSubmit, lockValue = false, builtinDomains = [] }) {
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
  const text = DIALOG_TEXT[list] || DIALOG_TEXT.blacklist
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
      if (!expiresMinutes) return setError('Срок: целое число минут больше нуля')
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
      title={text.title}
      description={text.description}
      size="sm"
    >
      <form id={`${ids}-form`} onSubmit={submit} className="space-y-4" noValidate>
        <div>
          <label className="label" htmlFor={`${ids}-v`}>
            {bot ? 'Имя бота или часть User-Agent' : domain ? 'Домен' : 'IP-адрес или CIDR'}
          </label>
          <input
            id={`${ids}-v`}
            className="input font-mono"
            value={value}
            readOnly={lockValue}
            onChange={(e) => setValue(e.target.value)}
            placeholder={bot ? 'AhrefsBot или MyScraper/1.0' : domain ? 'example.com' : '203.0.113.7 или 198.51.100.0/24'}
            aria-invalid={!!error}
            aria-describedby={error ? `${ids}-err` : undefined}
            data-autofocus={lockValue ? undefined : true}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <div>
          <label className="label" htmlFor={`${ids}-c`}>
            Комментарий (необязательно)
          </label>
          <input id={`${ids}-c`} className="input" value={comment} onChange={(e) => setComment(e.target.value)} maxLength={200} />
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label className="label" htmlFor={`${ids}-e`}>
              Срок действия
            </label>
            <select id={`${ids}-e`} className="input" value={expiry} onChange={(e) => setExpiry(e.target.value)}>
              {EXPIRY_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </div>
          {expiry === 'custom' ? (
            <div>
              <label className="label" htmlFor={`${ids}-m`}>
                Минут
              </label>
              <input id={`${ids}-m`} className="input" inputMode="numeric" value={custom} onChange={(e) => setCustom(e.target.value)} placeholder="90" />
            </div>
          ) : null}
        </div>
        {domain ? <p className="text-xs text-muted">Поддомены входят автоматически. Можно вставить адрес целиком: схема, порт, путь и «*.» отбрасываются.</p> : null}
        {bot ? (
          <p className="text-xs text-muted">Имя из каталога или любая часть заголовка User-Agent, от 3 символов. Регистр не важен.</p>
        ) : null}
        {error ? (
          <p id={`${ids}-err`} role="alert" className="text-sm text-danger">
            {error}
          </p>
        ) : null}
        <div className="flex justify-end gap-2 pt-1">
          <button type="button" className="btn" onClick={onClose}>
            Отмена
          </button>
          <button type="submit" className={`btn ${black ? 'btn-danger' : 'btn-primary'}`} disabled={busy}>
            {black ? 'Заблокировать' : 'Добавить'}
          </button>
        </div>
      </form>
    </Modal>
  )
}

/** Temporary ban with a custom duration. */
export function BanDialog({ open, onClose, ip, you, onSubmit }) {
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
    if (!m) return setError('Укажите целое число минут больше нуля')
    const self = selfBlockError(ip, you)
    if (self) return setError(self)
    setBusy(true)
    const ok = await onSubmit({ ip, minutes: m, reason: 'manual' })
    setBusy(false)
    if (ok) onClose()
  }

  return (
    <Modal open={open} onClose={onClose} title="Бан на свой срок" description={ip} size="sm">
      <form onSubmit={submit} className="space-y-4" noValidate>
        <div>
          <label className="label" htmlFor={`${ids}-m`}>
            Длительность, минут
          </label>
          <input id={`${ids}-m`} className="input" inputMode="numeric" value={minutes} onChange={(e) => setMinutes(e.target.value)} data-autofocus />
          <div className="mt-2 flex flex-wrap gap-1.5">
            {[...BAN_PRESETS, { minutes: 10080, label: '7 д' }].map((p) => (
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
            Отмена
          </button>
          <button type="submit" className="btn btn-danger" disabled={busy}>
            Забанить
          </button>
        </div>
      </form>
    </Modal>
  )
}

export function Empty({ children }) {
  return <p className="px-3 py-6 text-center text-sm text-muted">{children}</p>
}
