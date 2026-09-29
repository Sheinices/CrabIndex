// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { createContext, useCallback, useContext, useRef, useState } from 'react'
import { AlertTriangle } from 'lucide-react'
import { Modal } from './Modal.jsx'
import { useT } from '../lang/index.jsx'

const ConfirmContext = createContext(null)

/** Promise-based confirmation dialog (replaces window.confirm). */
export function ConfirmProvider({ children }) {
  const t = useT()
  const [state, setState] = useState(null)
  const resolver = useRef(null)

  const confirm = useCallback(
    (opts) => {
      return new Promise((resolve) => {
        resolver.current = resolve
        setState({ confirmLabel: t('confirm_ok'), cancelLabel: t('cancel'), danger: false, ...opts })
      })
    },
    [t],
  )

  const close = (value) => {
    resolver.current?.(value)
    resolver.current = null
    setState(null)
  }

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      <Modal
        open={!!state}
        onClose={() => close(false)}
        title={state?.title}
        size="sm"
        footer={
          <>
            <button type="button" className="btn" onClick={() => close(false)} data-autofocus>
              {state?.cancelLabel}
            </button>
            <button type="button" className={`btn ${state?.danger ? 'btn-danger' : 'btn-primary'}`} onClick={() => close(true)}>
              {state?.confirmLabel}
            </button>
          </>
        }
      >
        <div className="flex gap-3">
          {state?.danger ? <AlertTriangle className="mt-0.5 size-5 shrink-0 text-danger" aria-hidden="true" /> : null}
          <div className="space-y-2 text-sm">{state?.message}</div>
        </div>
      </Modal>
    </ConfirmContext.Provider>
  )
}

export function useConfirm() {
  const ctx = useContext(ConfirmContext)
  if (!ctx) throw new Error('useConfirm outside ConfirmProvider')
  return ctx
}
