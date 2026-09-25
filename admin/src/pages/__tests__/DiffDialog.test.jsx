import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { DiffDialog } from '../settings/DiffDialog.jsx'

const diff = {
  ok: true,
  changeCount: 3,
  diffs: [
    { path: 'listenport', oldValue: '9117', newValue: '9118', sensitive: false, change: 'modified' },
    { path: 'devkey', oldValue: 'old-secret', newValue: 'new-secret', sensitive: true, change: 'modified' },
    { path: 'Kinozal.cookie', oldValue: null, newValue: 'uid=42', sensitive: false, change: 'added' },
  ],
  validation: { ok: true, errors: [], warnings: [] },
}

describe('DiffDialog', () => {
  it('masks secrets and lists admin access changes', () => {
    render(<DiffDialog open diff={diff} accessChanges={['devkey']} onClose={vi.fn()} onConfirm={vi.fn()} />)
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByText('9118')).toBeInTheDocument()
    expect(dialog).not.toHaveTextContent('new-secret')
    expect(dialog).not.toHaveTextContent('old-secret')
    expect(dialog).not.toHaveTextContent('uid=42')
    expect(dialog).toHaveTextContent('Меняется доступ к админ-панели')
    expect(screen.getByRole('button', { name: 'Сохранить' })).toBeEnabled()
  })

  it('blocks saving when validation fails', () => {
    render(
      <DiffDialog
        open
        diff={{ ...diff, validation: { ok: false, error: 'listenport: 1-65535', errors: ['listenport: 1-65535'] } }}
        onClose={vi.fn()}
        onConfirm={vi.fn()}
      />,
    )
    expect(screen.getByRole('alert')).toHaveTextContent('listenport')
    expect(screen.getByRole('button', { name: 'Сохранить' })).toBeDisabled()
  })
})
