import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from 'react'
import { useBlocker, useSearchParams } from 'react-router'
import { CheckCircle2, ChevronDown, Code2, Copy, ExternalLink, FileCheck2, KeyRound, ListTree, RefreshCw, Save, Search, Sparkles } from 'lucide-react'
import * as apiClient from '../../lib/api.js'
import {
  adminAccessChanges,
  adminEntryUrl,
  deepClone,
  getByPath,
  setByPath,
  validateAdminSection,
  withAdminGroup,
} from '../../lib/config.js'
import { useToast } from '../../components/Toast.jsx'
import { useConfirm } from '../../components/Confirm.jsx'
import { Modal } from '../../components/Modal.jsx'
import { ErrorBox, PageHeader, Spinner } from '../../components/ui.jsx'
import { useTheme } from '../../hooks/useTheme.js'
import { formatDate } from '../../lib/format.js'
import { SettingsField } from './SettingsField.jsx'
import { ACCESS_LABELS, DiffDialog } from './DiffDialog.jsx'

const CodeEditor = lazy(() => import('./CodeEditor.jsx'))

function FieldGrid({ fields, data, onChange, prefix }) {
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      {(fields || []).map((f) => {
        const path = prefix ? `${prefix}.${f.key}` : f.key
        return <SettingsField key={path} field={f} path={path} value={getByPath(data, path)} onChange={(v) => onChange(path, v)} />
      })}
    </div>
  )
}

function TrackersGroup({ group, data, onChange }) {
  const [q, setQ] = useState('')
  const list = (group.trackers || []).filter((t) => {
    const s = q.trim().toLowerCase()
    if (!s) return true
    return t.title.toLowerCase().includes(s) || String(getByPath(data, `${t.id}.host`) ?? '').toLowerCase().includes(s)
  })
  return (
    <div className="space-y-3">
      <div className="relative max-w-sm">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
        <input type="search" className="input pl-9" placeholder="Поиск трекера или хоста" aria-label="Поиск трекера" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <p className="text-xs text-muted">
        Показано {list.length} из {(group.trackers || []).length}
      </p>
      {list.map((t) => (
        <details key={t.id} className="group rounded-xl border border-border">
          <summary className="flex cursor-pointer list-none items-center gap-3 px-4 py-3 [&::-webkit-details-marker]:hidden">
            <span className="font-medium">{t.title}</span>
            <span className="truncate font-mono text-xs text-muted">{String(getByPath(data, `${t.id}.host`) ?? '')}</span>
            <ChevronDown className="ml-auto size-4 shrink-0 transition-transform group-open:rotate-180" aria-hidden="true" />
          </summary>
          <div className="border-t border-border p-4">
            <FieldGrid fields={t.fields} data={data} onChange={onChange} prefix={t.id} />
          </div>
        </details>
      ))}
    </div>
  )
}

function SavedAccessDialog({ info, onClose }) {
  const toast = useToast()
  if (!info) return null
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(info.url)
      toast.info('Адрес скопирован')
    } catch {
      toast.error('Не удалось скопировать')
    }
  }
  const urlChanged = info.changes.some((k) => k === 'admin.path' || k === 'admin.token')
  return (
    <Modal
      open
      onClose={onClose}
      title="Доступ к админ-панели изменён"
      description="Конфигурация сохранена."
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Закрыть
          </button>
          {urlChanged && info.enabled ? (
            <a className="btn btn-primary" href={info.url}>
              <ExternalLink className="size-4" aria-hidden="true" /> Перейти по новому адресу
            </a>
          ) : null}
        </>
      }
    >
      <div className="space-y-3 text-sm">
        <ul className="list-disc pl-5 text-muted">
          {info.changes.map((k) => (
            <li key={k}>{ACCESS_LABELS[k] || k}</li>
          ))}
        </ul>
        {!info.enabled ? <p className="text-danger">Админ-панель отключена (admin.enable = false). После применения конфигурации она станет недоступна.</p> : null}
        <div>
          <p className="label">Адрес входа</p>
          <div className="flex gap-2">
            <code className="input overflow-x-auto font-mono text-xs whitespace-nowrap">{info.url}</code>
            <button type="button" className="btn" onClick={copy} aria-label="Скопировать адрес">
              <Copy className="size-4" aria-hidden="true" />
            </button>
          </div>
        </div>
        {urlChanged ? <p>Текущая сессия привязана к старому адресу - сохраните новый адрес, старый перестанет открываться.</p> : null}
        {info.changes.includes('devkey') ? <p>Для следующего входа используйте новый dev-ключ (devkey).</p> : null}
        <p className="text-xs text-muted">Адрес и ключ также выводит команда <code className="font-mono">crabindex admin</code>.</p>
      </div>
    </Modal>
  )
}

export default function SettingsPage() {
  const toast = useToast()
  const confirm = useConfirm()
  const { theme } = useTheme()
  // `?group=waf` deep-links to a settings group (e.g. from the WAF page).
  const [searchParams] = useSearchParams()
  const requestedGroup = searchParams.get('group')

  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState(null)
  const [busy, setBusy] = useState(false)
  const [meta, setMeta] = useState({})
  const [schema, setSchema] = useState(null)
  const [original, setOriginal] = useState({})
  const [formData, setFormData] = useState({})
  const [raw, setRaw] = useState('')
  const [format, setFormat] = useState('yaml')
  const [mode, setMode] = useState('form')
  const [activeGroup, setActiveGroup] = useState(requestedGroup)
  const [dirty, setDirty] = useState(false)
  const [rev, setRev] = useState(0)
  const [validation, setValidation] = useState(null)
  const [pending, setPending] = useState(null)
  const [accessInfo, setAccessInfo] = useState(null)

  const load = useCallback(async (fmt) => {
    setLoading(true)
    setLoadError(null)
    try {
      const res = await apiClient.getConfig(fmt)
      if (res?.ok === false) throw new Error(res.error || 'Не удалось загрузить конфигурацию')
      let sch = res.schema
      if (!sch) sch = (await apiClient.getConfigSchema())?.schema
      const s = withAdminGroup(sch || { groups: [] })
      setSchema(s)
      setOriginal(deepClone(res.data || {}))
      setFormData(deepClone(res.data || {}))
      setRaw(res.content || '')
      const f = res.displayFormat === 'json' || res.displayFormat === 'yaml' ? res.displayFormat : res.format === 'json' ? 'json' : 'yaml'
      setFormat(f)
      setMeta({ path: res.path, format: res.format, lastModifiedUtc: res.lastModifiedUtc, sensitiveFields: res.sensitiveFields, examplePath: res.examplePath })
      setActiveGroup((g) => (g && s.groups.some((x) => x.id === g) ? g : s.groups[0]?.id ?? null))
      setDirty(false)
      setValidation(null)
      setRev((r) => r + 1)
    } catch (e) {
      if (e?.status !== 401) setLoadError(e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    load()
  }, [load])

  useEffect(() => {
    if (!dirty) return undefined
    const onUnload = (e) => {
      e.preventDefault()
      e.returnValue = ''
    }
    window.addEventListener('beforeunload', onUnload)
    return () => window.removeEventListener('beforeunload', onUnload)
  }, [dirty])

  const blocker = useBlocker(({ currentLocation, nextLocation }) => dirty && currentLocation.pathname !== nextLocation.pathname)
  useEffect(() => {
    if (blocker.state !== 'blocked') return
    confirm({
      title: 'Несохранённые изменения',
      message: <p>Уйти со страницы? Изменения конфигурации будут потеряны.</p>,
      confirmLabel: 'Уйти',
      danger: true,
    }).then((ok) => (ok ? blocker.proceed() : blocker.reset()))
  }, [blocker, confirm])

  const onFieldChange = useCallback((path, value) => {
    setFormData((d) => setByPath(d, path, value))
    setDirty(true)
  }, [])

  /** Current editor state as `{ data, format }` (raw mode is parsed server-side). */
  const payload = async () => {
    if (mode === 'form') return { data: deepClone(formData), format }
    const parsed = await apiClient.parseConfig({ content: raw, format })
    if (!parsed?.ok || !parsed.data) throw new Error(parsed?.error || 'Не удалось разобрать конфигурацию')
    return { data: parsed.data, format }
  }

  const guard = async (fn) => {
    setBusy(true)
    try {
      await fn()
    } catch (e) {
      if (e?.status !== 401) toast.error('Ошибка', e?.message || String(e))
    } finally {
      setBusy(false)
    }
  }

  const switchMode = (next) =>
    guard(async () => {
      if (next === mode) return
      if (next === 'raw') {
        const r = await apiClient.renderConfig({ data: formData, format })
        if (!r?.ok) throw new Error(r?.error || 'Не удалось сформировать текст')
        setRaw(r.content || '')
      } else {
        const p = await apiClient.parseConfig({ content: raw, format })
        if (!p?.ok || !p.data) throw new Error(p?.error || 'Не удалось разобрать конфигурацию')
        setFormData(p.data)
        setRev((x) => x + 1)
      }
      setMode(next)
    })

  const changeFormat = (next) =>
    guard(async () => {
      if (next === format) return
      if (mode === 'raw') {
        const { data } = await payload()
        const r = await apiClient.renderConfig({ data, format: next })
        if (!r?.ok) throw new Error(r?.error || 'Не удалось сформировать текст')
        setRaw(r.content || '')
      }
      setFormat(next)
      setDirty(true)
    })

  const validate = () =>
    guard(async () => {
      const p = await payload()
      const res = await apiClient.validateConfig(p)
      const adminErrors = validateAdminSection(p.data)
      const merged = { ...res, errors: [...(res?.errors || []), ...adminErrors], ok: res?.ok !== false && adminErrors.length === 0 }
      setValidation(merged)
      if (merged.ok && !merged.warnings?.length) toast.success('Конфигурация корректна')
      else if (merged.ok) toast.info('Корректна, есть предупреждения')
      else toast.error('В конфигурации есть ошибки')
    })

  const formatDoc = () =>
    guard(async () => {
      const p = await payload()
      const res = await apiClient.formatConfig(p)
      if (!res?.ok) throw new Error(res?.error || 'Не удалось отформатировать')
      if (res.data) setFormData(res.data)
      setRaw(res.content || '')
      setMode('raw')
      setDirty(true)
      setRev((x) => x + 1)
      toast.success('Отформатировано')
    })

  const prepareSave = () =>
    guard(async () => {
      const p = await payload()
      const diff = await apiClient.diffConfig(p)
      if (diff?.ok === false && diff.error) throw new Error(diff.error)
      setPending({ payload: p, diff, adminErrors: validateAdminSection(p.data), access: adminAccessChanges(original, p.data) })
    })

  const confirmSave = () =>
    guard(async () => {
      const { payload: p, access } = pending
      const res = await apiClient.saveConfig(p)
      if (!res?.ok) throw new Error(res?.error || res?.message || 'Не удалось сохранить')
      setPending(null)
      toast.success('Сохранено', res.message)
      if (access.length) {
        setAccessInfo({ changes: access, url: adminEntryUrl(p.data), enabled: getByPath(p.data, 'admin.enable') !== false })
      }
      setDirty(false)
      if (!access.some((k) => k === 'admin.path' || k === 'admin.token')) await load(format)
    })

  const reload = async () => {
    if (dirty) {
      const ok = await confirm({ title: 'Перезагрузить конфигурацию?', message: <p>Несохранённые изменения будут потеряны.</p>, danger: true, confirmLabel: 'Перезагрузить' })
      if (!ok) return
    }
    load(format)
  }

  const groups = useMemo(() => schema?.groups || [], [schema])
  const group = groups.find((g) => g.id === activeGroup) || groups[0]

  if (loading && !schema) return <Spinner className="size-5" label="Загрузка конфигурации…" />

  return (
    <>
      <PageHeader
        title="Настройки"
        description={
          <>
            <span className="font-mono">{meta.path || 'init.yaml'}</span>
            {meta.lastModifiedUtc ? ` · изменён ${formatDate(meta.lastModifiedUtc)}` : ''}
            {dirty ? <span className="ml-2 font-medium text-warn">· есть несохранённые изменения</span> : null}
          </>
        }
        actions={
          <>
            <button type="button" className="btn btn-sm" onClick={reload} disabled={busy}>
              <RefreshCw className="size-4" aria-hidden="true" /> Перезагрузить
            </button>
            <button type="button" className="btn btn-sm" onClick={formatDoc} disabled={busy}>
              <Sparkles className="size-4" aria-hidden="true" /> Форматировать
            </button>
            <button type="button" className="btn btn-sm" onClick={validate} disabled={busy}>
              <FileCheck2 className="size-4" aria-hidden="true" /> Проверить
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={prepareSave} disabled={busy}>
              {busy ? <Spinner /> : <Save className="size-4" aria-hidden="true" />} Сохранить…
            </button>
          </>
        }
      />
      <ErrorBox error={loadError} onRetry={() => load(format)} />

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div role="group" aria-label="Режим редактора" className="inline-flex rounded-lg border border-border p-0.5">
          {[
            { id: 'form', label: 'Форма', icon: ListTree },
            { id: 'raw', label: 'Текст', icon: Code2 },
          ].map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              aria-pressed={mode === id}
              onClick={() => switchMode(id)}
              disabled={busy}
              className={`inline-flex items-center gap-2 rounded-md px-3 py-1.5 text-sm ${mode === id ? 'bg-brand text-white' : 'text-muted hover:text-fg'}`}
            >
              <Icon className="size-4" aria-hidden="true" /> {label}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-2 text-sm">
          <span className="text-muted">Формат</span>
          <select className="input w-auto py-1.5" value={format} onChange={(e) => changeFormat(e.target.value)} disabled={busy}>
            <option value="yaml">YAML</option>
            <option value="json">JSON</option>
          </select>
        </label>
        <p className="flex items-center gap-1.5 text-xs text-muted">
          <KeyRound className="size-3.5 text-accent" aria-hidden="true" />
          Изменение <code className="font-mono">admin.path</code>, <code className="font-mono">admin.token</code> или <code className="font-mono">devkey</code> меняет адрес входа или ключ.
        </p>
      </div>

      {validation ? (
        <div
          role="status"
          className={`mb-4 rounded-xl border px-4 py-3 text-sm ${validation.ok ? 'border-ok/40 bg-ok/10' : 'border-danger/40 bg-danger/10 text-danger'}`}
        >
          <p className="flex items-center gap-2 font-medium">
            {validation.ok ? <CheckCircle2 className="size-4 text-ok" aria-hidden="true" /> : null}
            {validation.ok ? 'Конфигурация корректна' : 'Ошибки валидации'}
          </p>
          {[...(validation.errors || []), ...(validation.warnings || []).map((w) => `⚠ ${w}`)].length ? (
            <ul className="mt-1 list-disc pl-5">
              {(validation.errors || []).map((e) => (
                <li key={e}>{e}</li>
              ))}
              {(validation.warnings || []).map((w) => (
                <li key={w} className="text-warn">
                  {w}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      {mode === 'raw' ? (
        <Suspense fallback={<Spinner label="Загрузка редактора…" />}>
          <CodeEditor
            value={raw}
            format={format}
            theme={theme}
            label="Конфигурация"
            onChange={(v) => {
              setRaw(v)
              setDirty(true)
            }}
          />
        </Suspense>
      ) : (
        <div className="grid gap-6 lg:grid-cols-[14rem_1fr]">
          <nav aria-label="Группы настроек">
            <label className="lg:hidden">
              <span className="sr-only">Группа</span>
              <select className="input" value={group?.id || ''} onChange={(e) => setActiveGroup(e.target.value)}>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.title}
                  </option>
                ))}
              </select>
            </label>
            <ul className="hidden space-y-0.5 lg:block">
              {groups.map((g) => (
                <li key={g.id}>
                  <button
                    type="button"
                    onClick={() => setActiveGroup(g.id)}
                    aria-current={group?.id === g.id ? 'true' : undefined}
                    className={`w-full rounded-lg px-3 py-2 text-left text-sm ${group?.id === g.id ? 'bg-brand/15 font-medium text-accent' : 'text-muted hover:bg-surface-2 hover:text-fg'}`}
                  >
                    {g.title}
                    {g.id === 'admin' ? <KeyRound className="ml-2 inline size-3.5" aria-hidden="true" /> : null}
                  </button>
                </li>
              ))}
            </ul>
          </nav>
          {group ? (
            <section className="card min-w-0 p-5" aria-labelledby="settings-group-title" key={`${group.id}-${rev}`}>
              <h2 id="settings-group-title" className="font-semibold">
                {group.title}
              </h2>
              {group.description ? <p className="mt-1 mb-4 text-sm text-muted">{group.description}</p> : <div className="mb-4" />}
              {group.id === 'admin' ? (
                <div className="mb-4 rounded-lg border border-brand/40 bg-brand/10 px-3 py-2 text-sm">
                  Текущий адрес входа: <code className="font-mono text-xs break-all">{adminEntryUrl(original).replace(/\?.*$/, (m) => (m.length > 1 ? '?••••••' : ''))}</code>
                  <br />
                  <span className="text-muted">После изменения пути или токена откроется только новый адрес.</span>
                </div>
              ) : null}
              {group.trackers ? (
                <TrackersGroup group={group} data={formData} onChange={onFieldChange} />
              ) : (
                <FieldGrid fields={group.fields} data={formData} onChange={onFieldChange} />
              )}
            </section>
          ) : (
            <p className="text-sm text-muted">Схема конфигурации пуста.</p>
          )}
        </div>
      )}

      <DiffDialog
        open={!!pending}
        diff={pending?.diff}
        extraErrors={pending?.adminErrors}
        accessChanges={pending?.access}
        sensitiveFields={meta.sensitiveFields}
        busy={busy}
        onClose={() => setPending(null)}
        onConfirm={confirmSave}
      />
      <SavedAccessDialog info={accessInfo} onClose={() => setAccessInfo(null)} />
    </>
  )
}
