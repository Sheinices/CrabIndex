import { createContext, useCallback, useContext, useState } from 'react'
import { Copy } from 'lucide-react'
import { Modal } from './Modal.jsx'
import { useToast } from './Toast.jsx'
import { prettyResponse } from '../lib/format.js'

const ResultContext = createContext(null)

/**
 * Runs admin actions and shows the server's text/JSON response: a toast with
 * a short preview and a drawer with the full body.
 */
export function ResultProvider({ children }) {
  const toast = useToast()
  const [result, setResult] = useState(null)

  const show = useCallback((title, body, ok = true) => setResult({ title, body: prettyResponse(body), ok }), [])

  const run = useCallback(
    async (title, fn) => {
      try {
        const res = await fn()
        const body = res && typeof res === 'object' && 'data' in res && 'status' in res ? res.data : res
        const text = prettyResponse(body)
        const preview = text.length > 160 ? `${text.slice(0, 160)}…` : text
        toast.success(title, preview || 'Готово')
        if (text.length > 160 || text.includes('\n')) setResult({ title, body: text, ok: true })
        return { ok: true, body }
      } catch (e) {
        const text = prettyResponse(e?.body) || e?.message || String(e)
        if (e?.status !== 401) {
          toast.error(title, e?.message || 'Ошибка')
          setResult({ title, body: text, ok: false })
        }
        return { ok: false, error: e }
      }
    },
    [toast],
  )

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(result?.body || '')
      toast.info('Скопировано')
    } catch {
      toast.error('Не удалось скопировать')
    }
  }

  return (
    <ResultContext.Provider value={{ run, show }}>
      {children}
      <Modal
        open={!!result}
        onClose={() => setResult(null)}
        title={result?.title}
        description={result?.ok ? 'Ответ сервера' : 'Ошибка'}
        variant="drawer"
        footer={
          <>
            <button type="button" className="btn" onClick={copy}>
              <Copy className="size-4" aria-hidden="true" /> Копировать
            </button>
            <button type="button" className="btn btn-primary" onClick={() => setResult(null)}>
              Закрыть
            </button>
          </>
        }
      >
        <pre className={`rounded-lg bg-bg p-3 font-mono text-xs leading-relaxed break-words whitespace-pre-wrap ${result?.ok ? '' : 'text-danger'}`}>
          {result?.body || '(пустой ответ)'}
        </pre>
      </Modal>
    </ResultContext.Provider>
  )
}

export function useResult() {
  const ctx = useContext(ResultContext)
  if (!ctx) throw new Error('useResult outside ResultProvider')
  return ctx
}
