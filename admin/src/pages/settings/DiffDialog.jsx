import { AlertTriangle, KeyRound, Save } from 'lucide-react'
import { Modal } from '../../components/Modal.jsx'
import { Spinner } from '../../components/ui.jsx'
import { maskDiffEntry } from '../../lib/config.js'

const CHANGE_LABEL = { added: 'добавлено', removed: 'удалено', modified: 'изменено' }
const CHANGE_TONE = { added: 'text-ok', removed: 'text-danger', modified: 'text-warn' }

export const ACCESS_LABELS = {
  'admin.enable': 'admin.enable - включение панели',
  'admin.path': 'admin.path - адрес панели',
  'admin.token': 'admin.token - токен в адресе входа',
  devkey: 'devkey - ключ входа',
}

export function DiffDialog({ open, diff, extraErrors = [], accessChanges = [], sensitiveFields, busy, onClose, onConfirm }) {
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
      title="Предпросмотр изменений"
      description={count ? `Изменений: ${count}. Секретные значения скрыты.` : 'Изменений нет.'}
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Отмена
          </button>
          <button type="button" className="btn btn-primary" onClick={onConfirm} disabled={!canSave || busy}>
            {busy ? <Spinner /> : <Save className="size-4" aria-hidden="true" />}
            Сохранить
          </button>
        </>
      }
    >
      <div className="space-y-4">
        {errors.length ? (
          <div role="alert" className="rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
            <p className="font-medium">Ошибки - сохранение невозможно:</p>
            <ul className="mt-1 list-disc pl-5">
              {errors.map((e) => (
                <li key={e}>{e}</li>
              ))}
            </ul>
          </div>
        ) : null}
        {validation?.warnings?.length ? (
          <div className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-sm text-warn">
            <p className="font-medium">Предупреждения:</p>
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
              <p className="font-medium">Меняется доступ к админ-панели</p>
              <ul className="mt-1 list-disc pl-5 text-muted">
                {accessChanges.map((k) => (
                  <li key={k}>{ACCESS_LABELS[k] || k}</li>
                ))}
              </ul>
              <p className="mt-1 text-muted">После сохранения будет показан новый адрес входа. Сохраните его - старый перестанет работать.</p>
            </div>
          </div>
        ) : null}
        {entries.length ? (
          <div className="table-wrap">
            <table className="table text-xs">
              <thead>
                <tr>
                  <th scope="col">Ключ</th>
                  <th scope="col">Было</th>
                  <th scope="col">Стало</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((e) => (
                  <tr key={e.path}>
                    <td className="font-mono break-all">
                      {e.path}
                      <span className={`ml-2 whitespace-nowrap ${CHANGE_TONE[e.change] || 'text-muted'}`}>{CHANGE_LABEL[e.change] || e.change}</span>
                      {e.sensitive ? <AlertTriangle className="ml-1 inline size-3 text-warn" aria-label="секретное значение" /> : null}
                    </td>
                    <td className="max-w-56 font-mono break-all text-muted">{e.oldText}</td>
                    <td className="max-w-56 font-mono break-all">{e.newText}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-muted">Конфигурация совпадает с сохранённой.</p>
        )}
      </div>
    </Modal>
  )
}
