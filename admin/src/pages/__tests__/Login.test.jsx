// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { LoginPage } from '../Login.jsx'
import { AuthProvider } from '../../components/Auth.jsx'
import { LangProvider } from '../../lang/index.jsx'
import { setBaseForTests } from '../../lib/base.js'

function stubFetch(loginStatus, loginBody) {
  const fn = vi.fn(async (url) => {
    if (String(url).endsWith('/api/session')) {
      return new Response(JSON.stringify({ authenticated: false, loginEnabled: true }), { status: 200, headers: { 'content-type': 'application/json' } })
    }
    return new Response(JSON.stringify(loginBody), { status: loginStatus, headers: { 'content-type': 'application/json' } })
  })
  vi.stubGlobal('fetch', fn)
  return fn
}

function renderLogin() {
  setBaseForTests('/admin')
  // Pin the UI language to Russian so assertions match the base dictionary regardless of the
  // test env locale (this env has no localStorage, so the panel falls back to navigator.language).
  Object.defineProperty(navigator, 'language', { value: 'ru', configurable: true })
  return render(
    <LangProvider>
      <AuthProvider>
        <LoginPage />
      </AuthProvider>
    </LangProvider>,
  )
}

describe('LoginPage', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('shows an error for a wrong key (401)', async () => {
    const fetch = stubFetch(401, { ok: false, error: 'invalid key' })
    renderLogin()
    await userEvent.type(screen.getByLabelText('Dev-ключ'), 'wrong')
    await userEvent.click(screen.getByRole('button', { name: 'Войти' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Неверный ключ')
    const call = fetch.mock.calls.find(([u]) => String(u).endsWith('/api/login'))
    expect(call[1].headers['X-Crab-Admin']).toBe('1')
    expect(JSON.parse(call[1].body)).toEqual({ devkey: 'wrong' })
  })

  it('shows the rate-limit message on 429', async () => {
    stubFetch(429, { ok: false })
    renderLogin()
    await userEvent.type(screen.getByLabelText('Dev-ключ'), 'x')
    await userEvent.click(screen.getByRole('button', { name: 'Войти' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(/Слишком много попыток, подождите/)
  })

  it('toggles key visibility', async () => {
    stubFetch(200, { ok: true })
    renderLogin()
    const input = screen.getByLabelText('Dev-ключ')
    expect(input).toHaveAttribute('type', 'password')
    await userEvent.click(screen.getByRole('button', { name: 'Показать ключ' }))
    expect(input).toHaveAttribute('type', 'text')
  })
})
