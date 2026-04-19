import { beforeEach, describe, expect, it } from 'vitest'

import { useSelectionStore } from '@/lib/stores/selectionStore'

describe('selectionStore', () => {
  beforeEach(() => {
    const s = useSelectionStore.getState()
    s.setPage(null)
    s.clear()
  })

  it('setPage stores the id and resets node selection', () => {
    const s = useSelectionStore.getState()
    s.select('n-1')
    s.setPage('p-1')
    expect(useSelectionStore.getState().pageId).toBe('p-1')
    expect(useSelectionStore.getState().nodeIds.size).toBe(0)
  })

  it('single select replaces the set', () => {
    const s = useSelectionStore.getState()
    s.select('a')
    s.select('b')
    expect([...useSelectionStore.getState().nodeIds]).toEqual(['b'])
  })

  it('additive select toggles entries', () => {
    const s = useSelectionStore.getState()
    s.select('a', true)
    s.select('b', true)
    s.select('a', true) // re-add toggles off
    expect([...useSelectionStore.getState().nodeIds]).toEqual(['b'])
  })

  it('selectMany replaces the set', () => {
    const s = useSelectionStore.getState()
    s.select('x')
    s.selectMany(['a', 'b'])
    expect(useSelectionStore.getState().nodeIds).toEqual(new Set(['a', 'b']))
  })

  it('deselect removes a specific id', () => {
    const s = useSelectionStore.getState()
    s.selectMany(['a', 'b'])
    s.deselect('a')
    expect(useSelectionStore.getState().nodeIds).toEqual(new Set(['b']))
  })

  it('clear wipes node selection but keeps page', () => {
    const s = useSelectionStore.getState()
    s.setPage('p-1')
    s.selectMany(['a', 'b'])
    s.clear()
    expect(useSelectionStore.getState().pageId).toBe('p-1')
    expect(useSelectionStore.getState().nodeIds.size).toBe(0)
  })

  it('isSelected reports membership', () => {
    const s = useSelectionStore.getState()
    s.select('a')
    expect(s.isSelected('a')).toBe(true)
    expect(s.isSelected('b')).toBe(false)
  })
})
