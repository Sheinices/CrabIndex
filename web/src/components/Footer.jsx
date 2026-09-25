import { Send } from 'lucide-react'
import { useEffect, useState } from 'react'
import { getVersion } from '../lib/api.js'
import { useT } from '../context.js'

function GithubMark(props) {
  return (
    <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden {...props}>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

export function Footer() {
  const t = useT()
  const [version, setVersion] = useState('')

  useEffect(() => {
    const ctrl = new AbortController()
    getVersion({ signal: ctrl.signal })
      .then((v) => setVersion(String(v?.version || '')))
      .catch(() => {})
    return () => ctrl.abort()
  }, [])

  const link = 'inline-flex items-center gap-1.5 rounded-md text-muted transition-colors hover:text-fg'

  return (
    <footer className="border-t border-line">
      <div className="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-x-6 gap-y-3 px-4 py-6 text-sm sm:px-6">
        <div className="flex items-center gap-2 text-faint">
          <img src="/img/icon-64.png" alt="" width="18" height="18" className="size-[18px] opacity-80" />
          <span>CrabIndex</span>
          {version && <span className="font-mono text-xs">{t('footer.version', { v: version })}</span>}
        </div>
        <div className="flex items-center gap-5">
          <a className={link} href="https://github.com/sheinices/crabindex" target="_blank" rel="noopener noreferrer">
            <GithubMark className="size-4" />
            {t('footer.github')}
          </a>
          <a className={link} href="https://t.me/prisma_party" target="_blank" rel="noopener noreferrer">
            <Send className="size-4" aria-hidden />
            {t('footer.telegram')}
          </a>
        </div>
      </div>
    </footer>
  )
}
