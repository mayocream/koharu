import { describe, expect, it } from 'vitest'
import type { Document } from '@/types'
import {
  boundsToRegion,
  clampToDocument,
  expandBounds,
  withMargin,
} from './stroke-session'

const document: Document = {
  id: 'doc-1',
  path: '/tmp/doc-1.png',
  name: 'doc-1.png',
  image: 'https://example.com/doc-1.png',
  width: 100,
  height: 80,
  textBlocks: [],
}

describe('stroke session geometry', () => {
  it('clamps pointers to document bounds', () => {
    expect(clampToDocument({ x: -5, y: 120 }, document)).toEqual({
      x: 0,
      y: 80,
    })
  })

  it('expands bounds with brush radius', () => {
    expect(
      expandBounds(
        {
          minX: 10,
          minY: 10,
          maxX: 20,
          maxY: 20,
        },
        { x: 40, y: 12 },
        6,
      ),
    ).toEqual({
      minX: 10,
      minY: 6,
      maxX: 46,
      maxY: 20,
    })
  })

  it('converts stroke bounds into clipped regions', () => {
    expect(
      boundsToRegion(
        {
          minX: -3.2,
          minY: 5.4,
          maxX: 12.7,
          maxY: 15.1,
        },
        document,
      ),
    ).toEqual({
      x: 0,
      y: 5,
      width: 13,
      height: 11,
    })
  })

  it('adds inpaint margin while staying inside the document', () => {
    expect(
      withMargin(
        {
          minX: 5,
          minY: 4,
          maxX: 14,
          maxY: 12,
        },
        20,
        document,
      ),
    ).toEqual({
      x: 1,
      y: 0,
      width: 17,
      height: 16,
    })
  })
})
