import { describe, expect, it } from 'vitest'
import { cleanVersion, isBusy, notesLines, reachedTarget, stageLabel } from './update.js'

describe('update helpers', () => {
  it('labels stages', () => {
    expect(stageLabel('verifying')).toBe('Проверка контрольной суммы')
    expect(stageLabel()).toBe('Ожидание')
  })

  it('knows busy stages', () => {
    expect(isBusy({ stage: 'installing' })).toBe(true)
    expect(isBusy({ stage: 'error' })).toBe(false)
    expect(isBusy(null)).toBe(false)
  })

  it('compares versions without v and build suffix', () => {
    expect(cleanVersion('v1.0.4+abc')).toBe('1.0.4')
    expect(reachedTarget('1.0.4', 'v1.0.4')).toBe(true)
    expect(reachedTarget('1.0.3', '1.0.4')).toBe(false)
    expect(reachedTarget('1.0.3', null)).toBe(false)
  })

  it('cleans release notes', () => {
    expect(notesLines('## What\'s new\n- **Fast** path\n\n')).toEqual(["What's new", '- Fast path'])
    expect(notesLines('a\nb\nc', 2)).toEqual(['a', 'b'])
  })
})
