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
import { useT } from '../../lang/index.jsx'

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
  const t = useT()
  const [q, setQ] = useState('')
  const list = (group.trackers || []).filter((tr) => {
    const s = q.trim().toLowerCase()
    if (!s) return true
    return tr.title.toLowerCase().includes(s) || String(getByPath(data, `${tr.id}.host`) ?? '').toLowerCase().includes(s)
  })
  return (
    <div className="space-y-3">
      <div className="relative max-w-sm">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
        <input type="search" className="input pl-9" placeholder={t('st_search_tracker_host')} aria-label={t('tr_search')} value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <p className="text-xs text-muted">{t('st_shown_of', { shown: list.length, total: (group.trackers || []).length })}</p>
      {list.map((tr) => (
        <details key={tr.id} className="group rounded-xl border border-border">
          <summary className="flex cursor-pointer list-none items-center gap-3 px-4 py-3 [&::-webkit-details-marker]:hidden">
            <span className="font-medium">{tr.title}</span>
            <span className="truncate font-mono text-xs text-muted">{String(getByPath(data, `${tr.id}.host`) ?? '')}</span>
            <ChevronDown className="ml-auto size-4 shrink-0 transition-transform group-open:rotate-180" aria-hidden="true" />
          </summary>
          <div className="border-t border-border p-4">
            <FieldGrid fields={tr.fields} data={data} onChange={onChange} prefix={tr.id} />
          </div>
        </details>
      ))}
    </div>
  )
}

function SavedAccessDialog({ info, onClose }) {
  const t = useT()
  const toast = useToast()
  if (!info) return null
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(info.url)
      toast.info(t('st_addr_copied'))
    } catch {
      toast.error(t('copy_failed'))
    }
  }
  const urlChanged = info.changes.some((k) => k === 'admin.path' || k === 'admin.token')
  return (
    <Modal
      open
      onClose={onClose}
      title={t('st_access_changed_title')}
      description={t('st_config_saved')}
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            {t('close')}
          </button>
          {urlChanged && info.enabled ? (
            <a className="btn btn-primary" href={info.url}>
              <ExternalLink className="size-4" aria-hidden="true" /> {t('st_go_new_addr')}
            </a>
          ) : null}
        </>
      }
    >
      <div className="space-y-3 text-sm">
        <ul className="list-disc pl-5 text-muted">
          {info.changes.map((k) => (
            <li key={k}>{t(ACCESS_LABELS[k] || k)}</li>
          ))}
        </ul>
        {!info.enabled ? <p className="text-danger">{t('st_admin_disabled_note')}</p> : null}
        <div>
          <p className="label">{t('st_entry_addr')}</p>
          <div className="flex gap-2">
            <code className="input overflow-x-auto font-mono text-xs whitespace-nowrap">{info.url}</code>
            <button type="button" className="btn" onClick={copy} aria-label={t('st_copy_addr')}>
              <Copy className="size-4" aria-hidden="true" />
            </button>
          </div>
        </div>
        {urlChanged ? <p>{t('st_session_old_addr')}</p> : null}
        {info.changes.includes('devkey') ? <p>{t('st_use_new_devkey')}</p> : null}
        <p className="text-xs text-muted">
          {t('st_cmd_note_prefix')} <code className="font-mono">crabindex admin</code>.
        </p>
      </div>
    </Modal>
  )
}

export default function SettingsPage() {
  const t = useT()
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
      if (res?.ok === false) throw new Error(res.error || t('st_err_load'))
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
  }, [t])

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
      title: t('st_unsaved_title'),
      message: <p>{t('st_unsaved_msg')}</p>,
      confirmLabel: t('st_leave'),
      danger: true,
    }).then((ok) => (ok ? blocker.proceed() : blocker.reset()))
  }, [blocker, confirm, t])

  const onFieldChange = useCallback((path, value) => {
    setFormData((d) => setByPath(d, path, value))
    setDirty(true)
  }, [])

  /** Current editor state as `{ data, format }` (raw mode is parsed server-side). */
  const payload = async () => {
    if (mode === 'form') return { data: deepClone(formData), format }
    const parsed = await apiClient.parseConfig({ content: raw, format })
    if (!parsed?.ok || !parsed.data) throw new Error(parsed?.error || t('st_err_parse'))
    return { data: parsed.data, format }
  }

  const guard = async (fn) => {
    setBusy(true)
    try {
      await fn()
    } catch (e) {
      if (e?.status !== 401) toast.error(t('error'), e?.message || String(e))
    } finally {
      setBusy(false)
    }
  }

  const switchMode = (next) =>
    guard(async () => {
      if (next === mode) return
      if (next === 'raw') {
        const r = await apiClient.renderConfig({ data: formData, format })
        if (!r?.ok) throw new Error(r?.error || t('st_err_render'))
        setRaw(r.content || '')
      } else {
        const p = await apiClient.parseConfig({ content: raw, format })
        if (!p?.ok || !p.data) throw new Error(p?.error || t('st_err_parse'))
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
        if (!r?.ok) throw new Error(r?.error || t('st_err_render'))
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
      if (merged.ok && !merged.warnings?.length) toast.success(t('st_valid_ok'))
      else if (merged.ok) toast.info(t('st_valid_warn'))
      else toast.error(t('st_valid_err'))
    })

  const formatDoc = () =>
    guard(async () => {
      const p = await payload()
      const res = await apiClient.formatConfig(p)
      if (!res?.ok) throw new Error(res?.error || t('st_err_format'))
      if (res.data) setFormData(res.data)
      setRaw(res.content || '')
      setMode('raw')
      setDirty(true)
      setRev((x) => x + 1)
      toast.success(t('st_formatted'))
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
      if (!res?.ok) throw new Error(res?.error || res?.message || t('st_err_save'))
      setPending(null)
      toast.success(t('st_saved'), res.message)
      if (access.length) {
        setAccessInfo({ changes: access, url: adminEntryUrl(p.data), enabled: getByPath(p.data, 'admin.enable') !== false })
      }
      setDirty(false)
      if (!access.some((k) => k === 'admin.path' || k === 'admin.token')) await load(format)
    })

  const reload = async () => {
    if (dirty) {
      const ok = await confirm({ title: t('st_reload_title'), message: <p>{t('st_reload_msg')}</p>, danger: true, confirmLabel: t('st_reload') })
      if (!ok) return
    }
    load(format)
  }

  const groups = useMemo(() => schema?.groups || [], [schema])
  const group = groups.find((g) => g.id === activeGroup) || groups[0]

  if (loading && !schema) return <Spinner className="size-5" label={t('st_loading_config')} />

  return (
    <>
      <PageHeader
        title={t('nav_settings')}
        description={
          <>
            <span className="font-mono">{meta.path || 'init.yaml'}</span>
            {meta.lastModifiedUtc ? t('st_modified', { date: formatDate(meta.lastModifiedUtc) }) : ''}
            {dirty ? <span className="ml-2 font-medium text-warn">{t('st_dirty')}</span> : null}
          </>
        }
        actions={
          <>
            <button type="button" className="btn btn-sm" onClick={reload} disabled={busy}>
              <RefreshCw className="size-4" aria-hidden="true" /> {t('st_reload')}
            </button>
            <button type="button" className="btn btn-sm" onClick={formatDoc} disabled={busy}>
              <Sparkles className="size-4" aria-hidden="true" /> {t('st_format')}
            </button>
            <button type="button" className="btn btn-sm" onClick={validate} disabled={busy}>
              <FileCheck2 className="size-4" aria-hidden="true" /> {t('st_validate')}
            </button>
            <button type="button" className="btn btn-primary btn-sm" onClick={prepareSave} disabled={busy}>
              {busy ? <Spinner /> : <Save className="size-4" aria-hidden="true" />} {t('st_save_ellipsis')}
            </button>
          </>
        }
      />
      <ErrorBox error={loadError} onRetry={() => load(format)} />

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div role="group" aria-label={t('st_editor_mode')} className="inline-flex rounded-lg border border-border p-0.5">
          {[
            { id: 'form', label: t('st_mode_form'), icon: ListTree },
            { id: 'raw', label: t('st_mode_raw'), icon: Code2 },
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
          <span className="text-muted">{t('st_format_label')}</span>
          <select className="input w-auto py-1.5" value={format} onChange={(e) => changeFormat(e.target.value)} disabled={busy}>
            <option value="yaml">YAML</option>
            <option value="json">JSON</option>
          </select>
        </label>
        <p className="flex items-center gap-1.5 text-xs text-muted">
          <KeyRound className="size-3.5 text-accent" aria-hidden="true" />
          {t('st_key_note_prefix')} <code className="font-mono">admin.path</code>, <code className="font-mono">admin.token</code> {t('or')}{' '}
          <code className="font-mono">devkey</code> {t('st_key_note_suffix')}
        </p>
      </div>

      {validation ? (
        <div
          role="status"
          className={`mb-4 rounded-xl border px-4 py-3 text-sm ${validation.ok ? 'border-ok/40 bg-ok/10' : 'border-danger/40 bg-danger/10 text-danger'}`}
        >
          <p className="flex items-center gap-2 font-medium">
            {validation.ok ? <CheckCircle2 className="size-4 text-ok" aria-hidden="true" /> : null}
            {validation.ok ? t('st_valid_ok') : t('st_valid_errors')}
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
        <Suspense fallback={<Spinner label={t('st_loading_editor')} />}>
          <CodeEditor
            value={raw}
            format={format}
            theme={theme}
            label={t('st_config_label')}
            onChange={(v) => {
              setRaw(v)
              setDirty(true)
            }}
          />
        </Suspense>
      ) : (
        <div className="grid gap-6 lg:grid-cols-[14rem_1fr]">
          <nav aria-label={t('st_groups')}>
            <label className="lg:hidden">
              <span className="sr-only">{t('st_group')}</span>
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
                  {t('st_current_entry')} <code className="font-mono text-xs break-all">{adminEntryUrl(original).replace(/\?.*$/, (m) => (m.length > 1 ? '?••••••' : ''))}</code>
                  <br />
                  <span className="text-muted">{t('st_after_change_note')}</span>
                </div>
              ) : null}
              {group.trackers ? (
                <TrackersGroup group={group} data={formData} onChange={onFieldChange} />
              ) : (
                <FieldGrid fields={group.fields} data={formData} onChange={onFieldChange} />
              )}
            </section>
          ) : (
            <p className="text-sm text-muted">{t('st_schema_empty')}</p>
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
