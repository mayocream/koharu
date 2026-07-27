import { describe, expect, it } from 'vitest'

import { draftFrame, zoomAtPoint } from '@/lib/koharu/geometry'

describe('editor geometry', () => {
  it('normalizes reverse-direction frame drafts', () => {
    expect(draftFrame([20, 30], [5, 10])).toMatchObject({ x: 5, y: 10, width: 15, height: 20 })
  })

  it('keeps the page point beneath the cursor while zooming', () => {
    const before = { zoom: 2, translation: [10, 20] as [number, number] }
    const after = zoomAtPoint(before, [110, 220], 4)
    expect(after.translation).toEqual([-90, -180])
  })
})
