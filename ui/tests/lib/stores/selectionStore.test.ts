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

  describe('page multi-selection', () => {
    it('initializes with empty selectedPageIds', () => {
      expect(useSelectionStore.getState().selectedPageIds.size).toBe(0)
    })

    it('setPage adds the active page to selectedPageIds and clears others if not already selected', () => {
      const s = useSelectionStore.getState()
      s.setSelectedPageIds(new Set(['p-1', 'p-2']))
      s.setPage('p-3')
      expect(useSelectionStore.getState().pageId).toBe('p-3')
      expect(useSelectionStore.getState().selectedPageIds).toEqual(new Set(['p-3']))
    })

    it('setPage keeps existing selection intact if set page is already selected', () => {
      const s = useSelectionStore.getState()
      s.setSelectedPageIds(new Set(['p-1', 'p-2']))
      // p-2 is already in selectedPageIds, so the selection should NOT be cleared/reset
      s.setPage('p-2')
      expect(useSelectionStore.getState().pageId).toBe('p-2')
      expect(useSelectionStore.getState().selectedPageIds).toEqual(new Set(['p-1', 'p-2']))
    })

    it('setSelectedPageIds updates page selections correctly', () => {
      const s = useSelectionStore.getState()
      s.setSelectedPageIds(new Set(['p-1', 'p-2']))
      expect(useSelectionStore.getState().selectedPageIds).toEqual(new Set(['p-1', 'p-2']))

      // Test functional updater
      s.setSelectedPageIds((prev) => {
        const next = new Set(prev)
        next.add('p-3')
        return next
      })
      expect(useSelectionStore.getState().selectedPageIds).toEqual(new Set(['p-1', 'p-2', 'p-3']))
    })

    it('setPage(null) clears selectedPageIds', () => {
      const s = useSelectionStore.getState()
      s.setSelectedPageIds(new Set(['p-1']))
      s.setPage(null)
      expect(useSelectionStore.getState().pageId).toBeNull()
      expect(useSelectionStore.getState().selectedPageIds.size).toBe(0)
    })
  })
})
