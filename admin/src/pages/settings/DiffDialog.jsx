// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { AlertTriangle, KeyRound, Save } from 'lucide-react'
import { Modal } from '../../components/Modal.jsx'
import { Spinner } from '../../components/ui.jsx'
import { maskDiffEntry } from '../../lib/config.js'
import { useT } from '../../lang/index.jsx'

// i18n keys (see lang/*.js); render with `t(MAP[k] || k)` so unknown values fall through as-is.
const CHANGE_LABEL = { added: 'change_added', removed: 'change_removed', modified: 'change_modified' }
const CHANGE_TONE = { added: 'text-ok', removed: 'text-danger', modified: 'text-warn' }

export const ACCESS_LABELS = {
  'admin.enable': 'access_admin_enable',
  'admin.path': 'access_admin_path',
  'admin.token': 'access_admin_token',
  devkey: 'access_devkey',
}

export function DiffDialog({ open, diff, extraErrors = [], accessChanges = [], sensitiveFields, busy, onClose, onConfirm }) {
  const t = useT()
  const entries = (diff?.diffs || []).map((d) => maskDiffEntry(d, sensitiveFields))
  const validation = diff?.validation
  const errors = [...(validation?.errors || []), ...extraErrors]
  if (validation?.error && !errors.includes(validation.error)) errors.unshift(validation.error)
  const canSave = validation?.ok !== false && extraErrors.length === 0 && entries.length > 0
  const count = diff?.changeCount ?? entries.length

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      title={t('dd_title')}
      description={count ? t('dd_desc_count', { n: count }) : t('dd_no_changes')}
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            {t('cancel')}
          </button>
          <button type="button" className="btn btn-primary" onClick={onConfirm} disabled={!canSave || busy}>
            {busy ? <Spinner /> : <Save className="size-4" aria-hidden="true" />}
            {t('save')}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        {errors.length ? (
          <div role="alert" className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
            <p className="font-medium">{t('dd_errors')}</p>
            <ul className="mt-1 list-disc pl-5">
              {errors.map((e) => (
                <li key={e}>{e}</li>
              ))}
            </ul>
          </div>
        ) : null}
        {validation?.warnings?.length ? (
          <div className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-sm text-warn">
            <p className="font-medium">{t('dd_warnings')}</p>
            <ul className="mt-1 list-disc pl-5">
              {validation.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          </div>
        ) : null}
        {accessChanges.length ? (
          <div className="flex gap-3 rounded-lg border border-brand/50 bg-brand/10 px-3 py-2 text-sm">
            <KeyRound className="mt-0.5 size-4 shrink-0 text-accent" aria-hidden="true" />
            <div>
              <p className="font-medium">{t('dd_access_changing')}</p>
              <ul className="mt-1 list-disc pl-5 text-muted">
                {accessChanges.map((k) => (
                  <li key={k}>{t(ACCESS_LABELS[k] || k)}</li>
                ))}
              </ul>
              <p className="mt-1 text-muted">{t('dd_access_note')}</p>
            </div>
          </div>
        ) : null}
        {entries.length ? (
          <div className="table-wrap">
            <table className="table text-xs">
              <thead>
                <tr>
                  <th scope="col">{t('dd_col_key')}</th>
                  <th scope="col">{t('dd_col_old')}</th>
                  <th scope="col">{t('dd_col_new')}</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((e) => (
                  <tr key={e.path}>
                    <td className="font-mono break-all">
                      {e.path}
                      <span className={`ml-2 whitespace-nowrap ${CHANGE_TONE[e.change] || 'text-muted'}`}>{t(CHANGE_LABEL[e.change] || e.change)}</span>
                      {e.sensitive ? <AlertTriangle className="ml-1 inline size-3 text-warn" aria-label={t('dd_secret')} /> : null}
                    </td>
                    <td className="max-w-56 font-mono break-all text-muted">{e.oldText}</td>
                    <td className="max-w-56 font-mono break-all">{e.newText}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-muted">{t('dd_same')}</p>
        )}
      </div>
    </Modal>
  )
}
