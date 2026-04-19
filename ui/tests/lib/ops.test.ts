import { describe, expect, it } from 'vitest'

import type { Node, Page } from '@/lib/api/schemas'
import { ops } from '@/lib/ops'

// The op constructors are pure — they shape JSON for the wire format. These
// tests pin the exact shape so accidental renames break loudly.

const samplePage = (): Page =>
  ({
    id: 'p-1',
    name: 'Page 1',
    width: 100,
    height: 200,
    nodes: {},
  }) as unknown as Page

const sampleNode = (): Node =>
  ({
    id: 'n-1',
    transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
    visible: true,
    kind: { text: {} },
  }) as unknown as Node

describe('ops constructors', () => {
  it('updateProjectMeta wraps patch', () => {
    expect(ops.updateProjectMeta({ name: 'x' })).toEqual({
      updateProjectMeta: { patch: { name: 'x' } },
    })
  })

  it('addPage carries at + page', () => {
    const page = samplePage()
    expect(ops.addPage(3, page)).toEqual({ addPage: { at: 3, page } })
  })

  it('removePage renames fields for wire format', () => {
    const page = samplePage()
    expect(ops.removePage('id-1', page, 2)).toEqual({
      removePage: { id: 'id-1', prev_page: page, prev_index: 2 },
    })
  })

  it('updatePage wraps patch', () => {
    expect(ops.updatePage('p-1', { name: 'renamed' })).toEqual({
      updatePage: { id: 'p-1', patch: { name: 'renamed' } },
    })
  })

  it('reorderPages preserves order and prev_order casing', () => {
    expect(ops.reorderPages(['a', 'b'], ['b', 'a'])).toEqual({
      reorderPages: { order: ['a', 'b'], prev_order: ['b', 'a'] },
    })
  })

  it('addNode carries page + at + node', () => {
    const node = sampleNode()
    expect(ops.addNode('p-1', 0, node)).toEqual({
      addNode: { page: 'p-1', at: 0, node },
    })
  })

  it('removeNode carries prev_node and prev_index', () => {
    const node = sampleNode()
    expect(ops.removeNode('p-1', 'n-1', node, 4)).toEqual({
      removeNode: { page: 'p-1', id: 'n-1', prev_node: node, prev_index: 4 },
    })
  })

  it('updateNode wraps patch', () => {
    expect(ops.updateNode('p-1', 'n-1', { visible: false })).toEqual({
      updateNode: { page: 'p-1', id: 'n-1', patch: { visible: false } },
    })
  })

  it('reorderNodes preserves order + prev_order', () => {
    expect(ops.reorderNodes('p-1', ['a'], ['a'])).toEqual({
      reorderNodes: { page: 'p-1', order: ['a'], prev_order: ['a'] },
    })
  })

  it('batch wraps label + ops', () => {
    const inner = ops.updatePage('p-1', {})
    expect(ops.batch('refactor', [inner])).toEqual({
      batch: { label: 'refactor', ops: [inner] },
    })
  })

  it('moveNode is shorthand for updateNode with transform', () => {
    const t = { x: 1, y: 2, width: 3, height: 4, rotationDeg: 0 }
    expect(ops.moveNode('p', 'n', t)).toEqual({
      updateNode: { page: 'p', id: 'n', patch: { transform: t } },
    })
  })

  it('setNodeVisible toggles visible', () => {
    expect(ops.setNodeVisible('p', 'n', true)).toEqual({
      updateNode: { page: 'p', id: 'n', patch: { visible: true } },
    })
  })

  it('updateText wraps text inside data', () => {
    expect(ops.updateText('p', 'n', { raw: 'hi' } as never)).toEqual({
      updateNode: { page: 'p', id: 'n', patch: { data: { text: { raw: 'hi' } } } },
    })
  })

  it('updateImage wraps image inside data', () => {
    expect(ops.updateImage('p', 'n', { opacity: 1 } as never)).toEqual({
      updateNode: { page: 'p', id: 'n', patch: { data: { image: { opacity: 1 } } } },
    })
  })

  it('updateMask wraps mask inside data', () => {
    expect(ops.updateMask('p', 'n', { opacity: 0.5 } as never)).toEqual({
      updateNode: { page: 'p', id: 'n', patch: { data: { mask: { opacity: 0.5 } } } },
    })
  })
})
