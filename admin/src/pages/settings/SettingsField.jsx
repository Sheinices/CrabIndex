import { useId, useState } from 'react'
import { Eye, EyeOff, Wand2 } from 'lucide-react'
import { generateToken, stringListToText, textToStringList } from '../../lib/config.js'
import { Toggle } from '../../components/ui.jsx'

function JsonField({ id, value, onChange, describedBy }) {
  const [text, setText] = useState(() => (value == null ? '' : JSON.stringify(value, null, 2)))
  const [error, setError] = useState('')
  return (
    <>
      <textarea
        id={id}
        className="input min-h-28 font-mono text-xs"
        value={text}
        spellCheck={false}
        aria-invalid={!!error}
        aria-describedby={describedBy}
        onChange={(e) => {
          const t = e.target.value
          setText(t)
          if (!t.trim()) {
            setError('')
            onChange(null)
            return
          }
          try {
            onChange(JSON.parse(t))
            setError('')
          } catch {
            setError('Некорректный JSON - значение не применено')
          }
        }}
      />
      {error ? <p className="mt-1 text-xs text-danger">{error}</p> : null}
    </>
  )
}

/** One schema-driven config field. `path` is the dotted key in the config document. */
export function SettingsField({ field, path, value, onChange }) {
  const id = useId()
  const descId = `${id}-desc`
  const [reveal, setReveal] = useState(false)
  const description = field.description ? (
    <p id={descId} className="mt-1 text-xs text-muted">
      {field.description}
    </p>
  ) : null
  const label = (
    <label htmlFor={id} className="label">
      {field.label || field.key} <span className="font-mono font-normal opacity-70">{path}</span>
    </label>
  )
  const describedBy = field.description ? descId : undefined

  if (field.type === 'bool') {
    return (
      <div className="rounded-lg border border-border p-3">
        <Toggle id={id} checked={!!value} onChange={onChange} label={<span className="font-medium">{field.label || field.key}</span>} />
        <p className="mt-1 font-mono text-[11px] text-muted">{path}</p>
        {description}
      </div>
    )
  }

  let control
  switch (field.type) {
    case 'int':
      control = (
        <input
          id={id}
          type="number"
          inputMode="numeric"
          className="input"
          min={field.min ?? undefined}
          max={field.max ?? undefined}
          value={value ?? ''}
          aria-describedby={describedBy}
          onChange={(e) => {
            const v = e.target.value
            onChange(v === '' ? null : Number.isNaN(Number(v)) ? v : Math.trunc(Number(v)))
          }}
        />
      )
      break
    case 'select':
      control = (
        <select id={id} className="input" value={value ?? ''} aria-describedby={describedBy} onChange={(e) => onChange(e.target.value || null)}>
          <option value="">- по умолчанию -</option>
          {(field.enumValues || []).map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      )
      break
    case 'stringList':
      control = (
        <textarea
          id={id}
          className="input min-h-20 font-mono text-xs"
          value={stringListToText(value)}
          placeholder="по одному значению на строку"
          aria-describedby={describedBy}
          onChange={(e) => onChange(textToStringList(e.target.value))}
        />
      )
      break
    case 'json':
      control = <JsonField id={id} value={value} onChange={onChange} describedBy={describedBy} />
      break
    case 'password': {
      const isToken = path === 'admin.token'
      control = (
        <div className="flex gap-2">
          <input
            id={id}
            type={reveal ? 'text' : 'password'}
            className="input font-mono"
            autoComplete="off"
            spellCheck={false}
            value={value ?? ''}
            aria-describedby={describedBy}
            onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
          />
          <button type="button" className="btn" onClick={() => setReveal((r) => !r)} aria-label={reveal ? 'Скрыть' : 'Показать'} aria-pressed={reveal}>
            {reveal ? <EyeOff className="size-4" aria-hidden="true" /> : <Eye className="size-4" aria-hidden="true" />}
          </button>
          {isToken ? (
            <button
              type="button"
              className="btn"
              onClick={() => {
                onChange(generateToken())
                setReveal(true)
              }}
              title="Сгенерировать новый токен"
              aria-label="Сгенерировать новый токен"
            >
              <Wand2 className="size-4" aria-hidden="true" />
            </button>
          ) : null}
        </div>
      )
      break
    }
    default:
      control = (
        <input
          id={id}
          type="text"
          className="input"
          value={value == null ? '' : typeof value === 'object' ? JSON.stringify(value) : value}
          aria-describedby={describedBy}
          onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
        />
      )
  }

  return (
    <div className={field.type === 'json' || field.type === 'stringList' ? 'sm:col-span-2' : ''}>
      {label}
      {control}
      {description}
    </div>
  )
}
