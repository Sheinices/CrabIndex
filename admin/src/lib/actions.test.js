import { describe, expect, it } from 'vitest'
import { ACTIONS, actionsFor, cleanParams, hasParseAll, supportsAction, TRACKER_ACTIONS, trackerLogName } from './actions.js'

const KNOWN = [
  'rutracker', 'rutor', 'kinozal', 'nnmclub', 'megapeer', 'bitru', 'toloka', 'mazepa', 'lostfilm', 'baibako', 'torrentby',
  'selezen', 'animelayer', 'anidub', 'anistar', 'anibelka', 'aniliberty', 'knaben', 'leproduction', 'viruseproject',
  'anifilm', 'korsars', 'ultradox', 'rudub', 'subsplease',
]

describe('tracker action map', () => {
  it('covers every known tracker and only defined actions', () => {
    expect(Object.keys(TRACKER_ACTIONS).sort()).toEqual([...KNOWN].sort())
    for (const ids of Object.values(TRACKER_ACTIONS)) {
      expect(ids).toContain('parse')
      for (const id of ids) expect(ACTIONS[id]).toBeDefined()
    }
  })

  it('exposes the ParseAll family only where routers define it', () => {
    expect(actionsFor('rutracker').map((a) => a.id)).toEqual(['parse', 'updatetasksparse', 'parsealltask', 'parselatest'])
    expect(hasParseAll('kinozal')).toBe(true)
    expect(hasParseAll('bitru')).toBe(false)
    expect(supportsAction('mazepa', 'parselatest')).toBe(false)
  })

  it('includes tracker-specific actions', () => {
    expect(supportsAction('bitru', 'backfill')).toBe(true)
    expect(actionsFor('knaben').map((a) => a.id)).toEqual(['parse', 'backfill', 'backfillstatus'])
    expect(supportsAction('lostfilm', 'parsepages')).toBe(true)
    expect(supportsAction('lostfilm', 'parseseasonpacks')).toBe(true)
    expect(supportsAction('subsplease', 'parseshows')).toBe(true)
    expect(supportsAction('animelayer', 'takelogin')).toBe(true)
    expect(actionsFor('unknown')).toEqual([])
  })

  it('attaches the right query params', () => {
    const lf = actionsFor('lostfilm').find((a) => a.id === 'parsepages')
    expect(lf.params.map((p) => p.name)).toEqual(['pagefrom', 'pageto'])
    expect(actionsFor('rutor').find((a) => a.id === 'parse').params.map((p) => p.name)).toEqual(['page'])
    expect(actionsFor('selezen')[0].params.map((p) => p.name)).toEqual(['parsefrom', 'parseto'])
    expect(actionsFor('rutracker').find((a) => a.id === 'parsealltask').params.map((p) => p.name)).toEqual(['cat', 'maxpages'])
  })

  it('drops empty params and builds log names', () => {
    expect(cleanParams({ pages: ' 3 ', cat: '', x: null })).toEqual({ pages: '3' })
    expect(trackerLogName('NNMClub')).toBe('nnmclub.log')
  })
})
