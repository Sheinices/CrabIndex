import { screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ITEMS } from '../test/fixtures.js'
import { locationRef, mockServer, renderApp } from '../test/render.jsx'

afterEach(() => vi.unstubAllGlobals())

function openServer({ configured = false, validKey = 'good' } = {}) {
  return mockServer((url) => {
    const key = url.searchParams.get('apikey') || ''
    switch (url.pathname) {
      case '/api/v1.0/conf':
        return [200, { configured, apikey: !configured || key === validKey, version: 't' }]
      case '/api/v1.0/torrents':
        if (configured && key !== validKey) return [401, {}]
        return [200, ITEMS]
      case '/version':
        return [200, { version: '9.9.9' }]
      default:
        return [404, {}]
    }
  })
}

describe('SearchPage', () => {
  it('shows the hero and runs a search that updates the URL', async () => {
    const calls = openServer()
    renderApp('/')
    const user = userEvent.setup()
    expect(screen.getByRole('heading', { name: /Поиск по всем трекерам/ })).toBeInTheDocument()

    await user.type(screen.getByRole('searchbox', { name: 'Поиск раздач' }), 'Динозавры{Enter}')

    expect(await screen.findAllByRole('article')).toHaveLength(3)
    expect(locationRef.current.search).toBe('?q=%D0%94%D0%B8%D0%BD%D0%BE%D0%B7%D0%B0%D0%B2%D1%80%D1%8B')
    const search = calls.find((u) => u.pathname === '/api/v1.0/torrents')
    expect(search.searchParams.get('search')).toBe('Динозавры')
    expect(search.searchParams.has('apikey')).toBe(false)
    // sorted by seeders: kinozal (40) first
    expect(within(screen.getAllByRole('article')[0]).getByText('Kinozal')).toBeInTheDocument()
    // saved to recent searches
    expect(JSON.parse(localStorage.getItem('crabindexRecentSearches'))).toEqual(['Динозавры'])
  })

  it('filters with chips, reflects filters in the URL and can reset them', async () => {
    openServer()
    renderApp('/?q=Динозавры')
    const user = userEvent.setup()
    await screen.findAllByRole('article')

    const sidebar = screen.getByRole('complementary', { name: 'Фильтры' })
    await user.click(within(sidebar).getByRole('button', { name: /^4K/ }))

    expect(screen.getAllByRole('article')).toHaveLength(1)
    expect(new URLSearchParams(locationRef.current.search).get('quality')).toBe('2160')

    await user.click(screen.getByRole('button', { name: 'Убрать фильтр 4K' }))
    expect(screen.getAllByRole('article')).toHaveLength(3)
  })

  it('shows an empty state when filters hide everything', async () => {
    openServer()
    renderApp('/?q=Динозавры&tracker=kinozal&quality=480')
    expect(await screen.findByText('Фильтры скрыли все результаты')).toBeInTheDocument()
    await userEvent.setup().click(screen.getAllByRole('button', { name: 'Сбросить фильтры' }).at(-1))
    expect(await screen.findAllByRole('article')).toHaveLength(3)
  })

  it('sorts via the select', async () => {
    openServer()
    renderApp('/?q=Динозавры')
    const user = userEvent.setup()
    await screen.findAllByRole('article')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Сортировка' }), 'size')
    expect(within(screen.getAllByRole('article')[0]).getByText('Kinozal')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'По убыванию' }))
    expect(within(screen.getAllByRole('article')[0]).getByText(/Динозавры \/ The Dinosaurs \[S01\] \(2026\) WEB/)).toBeInTheDocument()
    expect(new URLSearchParams(locationRef.current.search).get('dir')).toBe('asc')
  })

  it('focuses the search box with "/"', async () => {
    openServer()
    renderApp('/?q=Динозавры')
    await screen.findAllByRole('article')
    const input = screen.getByRole('searchbox', { name: 'Поиск раздач' })
    input.blur()
    await userEvent.setup().keyboard('/')
    expect(input).toHaveFocus()
  })

  it('asks for an API key when the server requires one and sends it afterwards', async () => {
    const calls = openServer({ configured: true })
    renderApp('/?q=Динозавры')
    const user = userEvent.setup()

    expect(await screen.findByRole('heading', { name: 'Для поиска нужен API-ключ.' })).toBeInTheDocument()
    // header control appears only when the server is configured with a key
    expect(screen.getByRole('button', { name: /API-ключ/ })).toBeInTheDocument()

    const main = screen.getByRole('main')
    await user.type(within(main).getByLabelText('API-ключ'), 'good')
    await user.click(within(main).getByRole('button', { name: 'Сохранить' }))

    expect(await screen.findAllByRole('article')).toHaveLength(3)
    expect(localStorage.getItem('api_key')).toBe('good')
    const last = calls.filter((u) => u.pathname === '/api/v1.0/torrents').at(-1)
    expect(last.searchParams.get('apikey')).toBe('good')
  })

  it('hides the API key control on open servers', async () => {
    const calls = openServer()
    renderApp('/')
    await waitFor(() => expect(calls.some((u) => u.pathname === '/api/v1.0/conf')).toBe(true))
    expect(await screen.findByText('Версия 9.9.9')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /API-ключ/ })).not.toBeInTheDocument()
  })

  it('shows a retryable error', async () => {
    let fail = true
    mockServer((url) => {
      if (url.pathname === '/api/v1.0/torrents') return fail ? [500, {}] : [200, ITEMS]
      return [200, {}]
    })
    renderApp('/?q=Динозавры')
    expect(await screen.findByText('Сервер ответил ошибкой 500.')).toBeInTheDocument()
    fail = false
    await userEvent.setup().click(screen.getByRole('button', { name: 'Повторить' }))
    expect(await screen.findAllByRole('article')).toHaveLength(3)
  })
})
