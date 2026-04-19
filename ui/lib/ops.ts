'use client'

import type {
  ImageDataPatch,
  MaskDataPatch,
  Node,
  NodeDataPatch,
  NodePatch,
  Op,
  Page,
  PagePatch,
  ProjectMetaPatch,
  TextDataPatch,
  Transform,
} from '@/lib/api/schemas'

/**
 * Typed `Op` constructors. Shimmed V1 api modules build ops through these
 * and submit via `applyCommand` from `@/lib/api/default/default`.
 *
 * Semantics for patch fields: missing = leave alone. `null` = clear (the
 * wire format encodes "present + cleared" as JSON null; our TS types honor
 * that via `field?: T | null`).
 */
export const ops = {
  updateProjectMeta(patch: ProjectMetaPatch): Op {
    return { updateProjectMeta: { patch } } as Op
  },

  addPage(at: number, page: Page): Op {
    return { addPage: { at, page } } as Op
  },

  removePage(id: string, page: Page, index: number): Op {
    return { removePage: { id, prev_page: page, prev_index: index } } as unknown as Op
  },

  updatePage(id: string, patch: PagePatch): Op {
    return { updatePage: { id, patch } } as Op
  },

  reorderPages(order: string[], prevOrder: string[]): Op {
    return { reorderPages: { order, prev_order: prevOrder } } as unknown as Op
  },

  addNode(page: string, at: number, node: Node): Op {
    return { addNode: { page, at, node } } as Op
  },

  removeNode(page: string, id: string, node: Node, index: number): Op {
    return { removeNode: { page, id, prev_node: node, prev_index: index } } as unknown as Op
  },

  updateNode(page: string, id: string, patch: NodePatch): Op {
    return { updateNode: { page, id, patch } } as Op
  },

  reorderNodes(page: string, order: string[], prevOrder: string[]): Op {
    return { reorderNodes: { page, order, prev_order: prevOrder } } as unknown as Op
  },

  batch(label: string, inner: Op[]): Op {
    return { batch: { label, ops: inner } } as Op
  },

  // --- convenience wrappers ---------------------------------------------

  moveNode(page: string, id: string, transform: Transform): Op {
    return ops.updateNode(page, id, { transform })
  },

  setNodeVisible(page: string, id: string, visible: boolean): Op {
    return ops.updateNode(page, id, { visible })
  },

  updateText(page: string, id: string, text: TextDataPatch): Op {
    return ops.updateNode(page, id, {
      data: { text } as NodeDataPatch,
    })
  },

  updateImage(page: string, id: string, image: ImageDataPatch): Op {
    return ops.updateNode(page, id, {
      data: { image } as NodeDataPatch,
    })
  },

  updateMask(page: string, id: string, mask: MaskDataPatch): Op {
    return ops.updateNode(page, id, {
      data: { mask } as NodeDataPatch,
    })
  },
}
