import { describe, expect, it } from 'vitest'
import { formatBytes, formatDuration, jobPercent, prettyResponse } from './format.js'

describe('format helpers', () => {
  it('formats durations and sizes', () => {
    expect(formatDuration(42)).toBe('42 с')
    expect(formatDuration(3700)).toBe('1 ч 1 мин')
    expect(formatDuration(90000)).toBe('1 д 1 ч')
    expect(formatBytes(512)).toBe('512 Б')
    expect(formatBytes(1536)).toBe('1.5 КБ')
  })

  it('computes job progress', () => {
    expect(jobPercent({ percent: 42 })).toBe(42)
    expect(jobPercent({ percent: null, pagesCompleted: 1, pagesTotal: 4 })).toBe(25)
    expect(jobPercent({ pagesTotal: 0 })).toBeNull()
  })

  it('pretty-prints JSON text responses', () => {
    expect(prettyResponse('{"a":1}')).toBe('{\n  "a": 1\n}')
    expect(prettyResponse('plain')).toBe('plain')
  })
})
