import { SearchX } from 'lucide-react'
import { Link } from 'react-router'
import { useT } from '../context.js'

export function NotFoundPage() {
  const t = useT()
  return (
    <div className="mx-auto flex max-w-md flex-col items-center px-4 py-24 text-center">
      <SearchX className="size-10 text-faint" aria-hidden />
      <h1 className="mt-4 text-xl font-semibold">{t('notFound.title')}</h1>
      <p className="mt-2 text-sm text-muted">{t('notFound.text')}</p>
      <Link to="/" className="btn btn-primary mt-6">
        {t('notFound.back')}
      </Link>
    </div>
  )
}
