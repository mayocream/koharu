import { act, fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Workspace } from '@/components/canvas/Workspace'
import { TooltipProvider } from '@/components/ui/tooltip'
import {
  koharuClient,
  useEditorStore,
  type CanvasInteraction,
  type Element,
  type UiCommand,
  type UiEvent,
} from '@/lib/koharu'

const element: Element = {
  id: 'element',
  frame: { x: 10, y: 20, width: 100, height: 50, angle_degrees: 0 },
  visible: true,
  opacity: 1,
  kind: {
    Region: {
      kind: 'Panel',
      polygon: [],
      mask_id: null,
      reading_order: null,
      predictions: [],
    },
  },
}

function installProject() {
  useEditorStore.setState({
    connection: 'connected',
    revision: 1,
    project: {
      id: 'project',
      name: 'Book',
      visible_page: 'page',
      can_undo: false,
      can_redo: false,
    },
    pages: [],
    page: {
      id: 'page',
      name: 'Page',
      size: { width: 1000, height: 1000 },
      source: 'source',
      assets: {
        clean: null,
        rendered: null,
        text_mask: null,
        coo_mask: null,
        bubble_mask: null,
        brush_mask: null,
      },
      elements: [element],
    },
    selectedElements: [],
    camera: { zoom: 1, translation: [0, 0], autoFit: false },
    tool: 'select',
  })
}

function renderWorkspace() {
  let listener: ((event: UiEvent) => void) | undefined
  const interactions: CanvasInteraction[] = []
  const commands: UiCommand[] = []
  vi.spyOn(koharuClient, 'subscribe').mockImplementation((next) => {
    listener = next
    return () => undefined
  })
  vi.spyOn(koharuClient, 'interact').mockImplementation((interaction) => {
    interactions.push(interaction)
  })
  vi.spyOn(koharuClient, 'fire').mockImplementation((command) => {
    commands.push(command)
  })
  render(
    <TooltipProvider>
      <Workspace />
    </TooltipProvider>,
  )
  return {
    surface: screen.getByLabelText('Koharu canvas'),
    interactions,
    commands,
    emit(event: UiEvent) {
      act(() => listener?.(event))
    },
  }
}

describe('canvas transforms', () => {
  it('forwards a move gesture to Rust and finishes it as one command', () => {
    installProject()
    const harness = renderWorkspace()

    fireEvent.pointerDown(harness.surface, {
      button: 0,
      pointerId: 7,
      clientX: 30,
      clientY: 40,
    })
    const hit = harness.interactions.find((interaction) => interaction.type === 'hit_test')
    expect(hit).toMatchObject({ type: 'hit_test', x: 30, y: 40 })
    if (!hit || hit.type !== 'hit_test') throw new Error('hit test was not sent')

    harness.emit({
      type: 'hit_test',
      id: hit.id,
      target: { type: 'element', element: element.id },
    })
    expect(harness.interactions).toContainEqual({
      type: 'begin_transform',
      elements: [element.id],
      target: { type: 'element', element: element.id },
      x: 30,
      y: 40,
    })

    fireEvent.pointerMove(harness.surface, { pointerId: 7, clientX: 55, clientY: 65 })
    expect(harness.interactions).toContainEqual({ type: 'update_transform', x: 55, y: 65 })

    fireEvent.pointerUp(harness.surface, { pointerId: 7, clientX: 58, clientY: 70 })
    expect(
      harness.interactions.filter((interaction) => interaction.type === 'update_transform').at(-1),
    ).toEqual({ type: 'update_transform', x: 58, y: 70 })
    expect(harness.commands).toEqual([{ type: 'finish_transform' }])
    expect(
      harness.interactions.some(
        (interaction) => interaction.type === 'set_overlays' && 'previews' in interaction,
      ),
    ).toBe(false)
  })

  it('cancels the Rust transform when pointer capture is cancelled', () => {
    installProject()
    const harness = renderWorkspace()

    fireEvent.pointerDown(harness.surface, {
      button: 0,
      pointerId: 9,
      clientX: 20,
      clientY: 25,
    })
    const hit = harness.interactions.find((interaction) => interaction.type === 'hit_test')
    if (!hit || hit.type !== 'hit_test') throw new Error('hit test was not sent')
    harness.emit({
      type: 'hit_test',
      id: hit.id,
      target: { type: 'handle', element: element.id, handle: 'east' },
    })

    fireEvent.pointerCancel(harness.surface, { pointerId: 9 })

    expect(harness.interactions.at(-2)).toEqual({ type: 'cancel_transform' })
    expect(harness.commands).toEqual([])
  })
})
