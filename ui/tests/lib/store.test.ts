import { describe, expect, it } from 'vitest'

import { dispatchEvent, useEditorStore } from '@/lib/koharu'

describe('projection reducer', () => {
  it('rejects deltas that do not start at the projected revision', () => {
    useEditorStore.setState({
      revision: 7,
      project: {
        id: 'p',
        name: 'Book',
        visible_page: null,
        can_undo: false,
        can_redo: false,
      },
    })
    expect(
      dispatchEvent({
        type: 'project_changed',
        from: 6,
        revision: 8,
        name: 'Book',
        page_order: [],
        pages: [],
        deleted_pages: [],
        visible_page: null,
        can_undo: false,
        can_redo: false,
      }),
    ).toBe(true)
    expect(useEditorStore.getState().revision).toBe(7)
  })

  it('cleans selection when a delta deletes elements and pages', () => {
    useEditorStore.setState({
      revision: 1,
      project: {
        id: 'p',
        name: 'Book',
        visible_page: 'page',
        can_undo: false,
        can_redo: false,
      },
      pages: [
        {
          id: 'page',
          name: 'Page',
          size: { width: 100, height: 100 },
          source: 'blob',
          clean: null,
          elements: 1,
        },
      ],
      page: {
        id: 'page',
        name: 'Page',
        size: { width: 100, height: 100 },
        source: 'blob',
        assets: {
          clean: null,
          rendered: null,
          text_mask: null,
          bubble_mask: null,
          brush_mask: null,
        },
        elements: [],
      },
      selectedElements: ['gone'],
      selectedPages: ['page'],
    })
    dispatchEvent({
      type: 'project_changed',
      from: 1,
      revision: 2,
      name: 'Book',
      page_order: [],
      pages: [],
      deleted_pages: ['page'],
      visible_page: null,
      can_undo: true,
      can_redo: false,
    })
    expect(useEditorStore.getState()).toMatchObject({
      page: null,
      selectedElements: [],
      selectedPages: [],
    })
  })

  it('retains failed downloads until they are dismissed', () => {
    useEditorStore.setState({ downloads: {} })
    dispatchEvent({
      type: 'download_changed',
      state: 'failed',
      id: 9,
      name: 'weights.bin',
      error: 'network unavailable',
    })
    expect(useEditorStore.getState().downloads[9]).toMatchObject({
      state: 'failed',
      name: 'weights.bin',
    })

    useEditorStore.getState().dismissDownload(9)
    expect(useEditorStore.getState().downloads[9]).toBeUndefined()
  })
})
