import { renderHook } from '@testing-library/react'
import { fireEvent } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { Node, Page, SceneSnapshot } from '@/lib/api/schemas'
import { queryClient } from '@/lib/queryClient'
import { useSelectionStore } from '@/lib/stores/selectionStore'
import { QueryClientProvider } from '@tanstack/react-query'

function textNode(id: string): Node {
  return {
    id,
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: { text: { raw: `t-${id}` } },
  } as unknown as Node
}

function seedScene(): SceneSnapshot {
  const page: Page = {
    id: 'p-1',
    name: 'P',
    width: 10,
    height: 10,
    nodes: { t1: textNode('t1'), t2: textNode('t2') },
  } as unknown as Page
  return {
    epoch: 1,
    scene: { pages: { 'p-1': page }, project: { name: 'P' } as never } as never,
  }
}

describe('useKeyboardShortcuts — Ctrl+A', () => {
  beforeEach(() => {
    useSelectionStore.getState().setPage(null)
    queryClient.clear()
  })

  it('Ctrl+A selects every text node on the active page', () => {
    queryClient.setQueryData(getGetSceneJsonQueryKey(), seedScene())
    useSelectionStore.getState().setPage('p-1')
    renderHook(() => useKeyboardShortcuts(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      ),
    })

    fireEvent.keyDown(window, { key: 'a', ctrlKey: true })

    expect([...useSelectionStore.getState().nodeIds].sort()).toEqual(['t1', 't2'])
  })

  it('Ctrl+A is a no-op while typing inside a textarea', () => {
    queryClient.setQueryData(getGetSceneJsonQueryKey(), seedScene())
    useSelectionStore.getState().setPage('p-1')
    renderHook(() => useKeyboardShortcuts(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      ),
    })

    const textarea = document.createElement('textarea')
    document.body.appendChild(textarea)
    textarea.focus()

    fireEvent.keyDown(textarea, { key: 'a', ctrlKey: true })

    expect(useSelectionStore.getState().nodeIds.size).toBe(0)

    document.body.removeChild(textarea)
  })
})
