import { describe, expect, it } from 'vitest'

import type { EntityView } from '@/lib/koharu'
import {
  draftFrame,
  entityFrame,
  hitTestEntities,
  scrollCamera,
  zoomAtPoint,
} from '@/lib/koharu/geometry'

const entity = (id: string, points: { x: number; y: number }[]): EntityView => ({
  id,
  parent: 'page',
  geometry: { points },
  visibility: { visible: true, opacity: 1 },
  image: 'image',
  source_text: null,
  translation: null,
  typography: null,
  region: null,
})

describe('editor geometry', () => {
  it('normalizes reverse-direction frame drafts', () => {
    expect(draftFrame([20, 30], [5, 10])).toMatchObject({ x: 5, y: 10, width: 15, height: 20 })
  })

  it('keeps the page point beneath the cursor while zooming', () => {
    const before = { zoom: 2, translation: [10, 20] as [number, number] }
    const after = zoomAtPoint(before, [110, 220], 4)
    expect(after.translation).toEqual([-90, -180])
  })

  it('scrolls the camera without changing zoom', () => {
    expect(scrollCamera({ zoom: 2, translation: [100, 200] }, [12, -8], 2)).toEqual({
      zoom: 2,
      translation: [76, 216],
    })
  })

  it('projects rotated scene rectangles into React control frames', () => {
    const rotated = entity('rotated', [
      { x: 40, y: 20 },
      { x: 80, y: 60 },
      { x: 60, y: 80 },
      { x: 20, y: 40 },
    ])
    const frame = entityFrame(rotated)
    expect(frame?.x).toBeCloseTo(21.715729)
    expect(frame?.y).toBeCloseTo(35.857864)
    expect(frame?.width).toBeCloseTo(56.568542)
    expect(frame?.height).toBeCloseTo(28.284271)
    expect(frame?.angle_degrees).toBeCloseTo(45)
  })

  it('hit-tests topmost selectable entities locally', () => {
    const back = entity('back', [
      { x: 0, y: 0 },
      { x: 100, y: 0 },
      { x: 100, y: 100 },
      { x: 0, y: 100 },
    ])
    const front = entity('front', [
      { x: 25, y: 25 },
      { x: 75, y: 25 },
      { x: 75, y: 75 },
      { x: 25, y: 75 },
    ])
    expect(hitTestEntities([back, front], [50, 50], () => true)?.id).toBe('front')
  })
})
