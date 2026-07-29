import { act, render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { CanvasOverlay } from '@/components/canvas/CanvasOverlay'
import type { PageView, TransformFrame } from '@/lib/koharu'

const moveable = vi.hoisted(() => ({
  props: null as Record<string, (event: unknown) => void> | null,
}))

vi.mock('react-moveable', () => ({
  default: (props: Record<string, (event: unknown) => void>) => {
    moveable.props = props
    return null
  },
}))

const page: PageView = {
  id: 'page',
  label: 'Page',
  size: { width: 1000, height: 1000 },
  assets: {
    source: 'source',
    clean: null,
    rendered: null,
    text_mask: null,
    coo_mask: null,
    bubble_mask: null,
    brush_mask: null,
  },
  entities: [
    {
      id: 'element',
      parent: 'page',
      geometry: {
        points: [
          { x: 10, y: 20 },
          { x: 110, y: 20 },
          { x: 110, y: 70 },
          { x: 10, y: 70 },
        ],
      },
      visibility: { visible: true, opacity: 1 },
      image: 'image',
      source_text: null,
      translation: null,
      typography: null,
      region: null,
    },
  ],
}

describe('React canvas controls', () => {
  it('forwards every resize and rotation sample as an absolute frame', () => {
    const frames: TransformFrame[][] = []
    const starts: TransformFrame[][] = []
    const end = vi.fn()
    const view = render(
      <CanvasOverlay
        page={page}
        camera={{ zoom: 2, translation: [0, 0] }}
        selected={['element']}
        hovered={null}
        previews={{}}
        draft={null}
        cursor={null}
        brushSize={20}
        showBrushCursor={false}
        onTransformStart={(elements) => starts.push(elements)}
        onTransformFrame={(elements) => frames.push(elements)}
        onTransformEnd={end}
      />,
    )
    const target = view.container.querySelector<HTMLElement>('[data-element="element"]')!
    const props = moveable.props!

    act(() => {
      props.onResizeStart({
        target,
        set: vi.fn(),
        dragStart: { set: vi.fn() },
      })
      props.onResize({
        target,
        width: 220,
        height: 110,
        drag: { beforeTranslate: [20, 10] },
      })
      props.onResize({
        target,
        width: 240,
        height: 120,
        drag: { beforeTranslate: [24, 12] },
      })
      props.onResizeEnd({ target })
    })

    expect(starts).toHaveLength(1)
    expect(frames).toEqual([
      [
        {
          element: 'element',
          frame: { x: 20, y: 25, width: 110, height: 55, angle_degrees: 0 },
        },
      ],
      [
        {
          element: 'element',
          frame: { x: 22, y: 26, width: 120, height: 60, angle_degrees: 0 },
        },
      ],
    ])

    act(() => {
      props.onRotateStart({
        target,
        set: vi.fn(),
        dragStart: { set: vi.fn() },
      })
      props.onRotate({ target, beforeRotation: 15 })
      props.onRotate({ target, beforeRotation: 30 })
      props.onRotateEnd({ target })
    })

    expect(starts).toHaveLength(2)
    expect(frames.slice(2).map((sample) => sample[0].frame.angle_degrees)).toEqual([15, 30])
    expect(end).toHaveBeenCalledTimes(2)
  })
})
