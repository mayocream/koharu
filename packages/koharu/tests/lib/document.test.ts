import { describe, expect, it } from 'vitest'

import { isEditableColorPixelLayer, isLockedLayer } from '@/lib/document'
import type { Layer } from '@/lib/protocol'

const colorLayer: Layer = {
  type: 'pixel',
  id: 'paint',
  parent: 'page',
  geometry: null,
  visibility: { visible: true, opacity: 1 },
  image: 'paint-image',
  name: 'Paint 1',
  format: { kind: 'color' },
}

describe('layer capabilities', () => {
  it('only treats direct color pixel children as raster-editable', () => {
    expect(isEditableColorPixelLayer(colorLayer, 'page')).toBe(true)
    expect(
      isEditableColorPixelLayer(
        {
          ...colorLayer,
          format: { kind: 'mask', channel: 'alpha', tint: [255, 64, 64, 255] },
        },
        'page',
      ),
    ).toBe(false)
    expect(isEditableColorPixelLayer({ ...colorLayer, parent: null }, 'page')).toBe(false)
    expect(isEditableColorPixelLayer({ ...colorLayer, parent: 'group' }, 'page')).toBe(false)
  })

  it('locks the page-owned root pixel without locking editable page children', () => {
    expect(isLockedLayer({ ...colorLayer, id: 'page', parent: null })).toBe(true)
    expect(isLockedLayer(colorLayer)).toBe(false)
  })
})
