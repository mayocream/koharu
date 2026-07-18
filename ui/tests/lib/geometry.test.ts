import { describe, expect, it } from 'vitest'

import { draftFrame, transformFrame, zoomAtPoint } from '@/lib/koharu/geometry'

describe('editor geometry', () => {
  it('normalizes reverse-direction frame drafts', () => {
    expect(draftFrame([20, 30], [5, 10])).toMatchObject({ x: 5, y: 10, width: 15, height: 20 })
  })

  it('resizes in rotated local coordinates', () => {
    const next = transformFrame(
      { x: 0, y: 0, width: 100, height: 50, angle_degrees: 90 },
      { type: 'handle', element: 'text', handle: 'east' },
      0,
      20,
    )
    expect(next.width).toBeCloseTo(120)
    expect(next.height).toBeCloseTo(50)
  })

  it('keeps the page point beneath the cursor while zooming', () => {
    const before = { zoom: 2, translation: [10, 20] as [number, number] }
    const after = zoomAtPoint(before, [110, 220], 4)
    expect(after.translation).toEqual([-90, -180])
  })
})
