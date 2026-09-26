import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ToastProvider } from '../../components/Toast.jsx'
import { ConfirmProvider } from '../../components/Confirm.jsx'
import { setBaseForTests } from '../../lib/base.js'
import { UpdatePage } from '../Update.jsx'

function stub(body) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body), { status: 200, headers: { 'content-type': 'application/json' } })),
  )
}

function renderPage() {
  setBaseForTests('/admin')
  return render(
    <ToastProvider>
      <ConfirmProvider>
        <UpdatePage />
      </ConfirmProvider>
    </ToastProvider>,
  )
}

const LATEST = { version: '1.0.4', name: 'v1.0.4', publishedAt: '2026-09-26T10:00:00Z', notes: '## New\n- thing', url: 'https://github.com/x' }

describe('UpdatePage', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('offers the update when a newer release can be installed', async () => {
    stub({ current: '1.0.3', available: true, canSelfUpdate: true, latest: LATEST, state: { stage: 'idle' } })
    renderPage()
    expect(await screen.findByRole('button', { name: /Обновить до 1\.0\.4/ })).toBeInTheDocument()
    expect(screen.getByText('Доступна версия 1.0.4')).toBeInTheDocument()
    expect(screen.getByText('- thing')).toBeInTheDocument()
  })

  it('explains why self-update is unavailable', async () => {
    stub({ current: '1.0.3', available: true, canSelfUpdate: false, reason: 'CrabIndex работает в Docker', latest: LATEST, state: { stage: 'idle' } })
    renderPage()
    expect(await screen.findByText('Обновление из панели недоступно')).toBeInTheDocument()
    expect(screen.getByText('CrabIndex работает в Docker')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Обновить до/ })).toBeNull()
  })

  it('says the version is current', async () => {
    stub({ current: '1.0.4', available: false, canSelfUpdate: true, latest: LATEST, state: { stage: 'idle' } })
    renderPage()
    expect(await screen.findByText('Установлена актуальная версия')).toBeInTheDocument()
  })
})
