import { act, fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Workspace } from '@/components/canvas/Workspace'
import { TooltipProvider } from '@/components/ui/tooltip'
import {
  koharuClient,
  useEditorStore,
  type CanvasInteraction,
  type EntityView,
  type UiCommand,
  type UiEvent,
} from '@/lib/koharu'

const element: EntityView = {
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
  region: { kind: 'panel', label: null },
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
      entities: [element],
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
  vi.spyOn(koharuClient, 'command').mockImplementation(async (command) => {
    commands.push(command)
    return 'accepted'
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
    expect(harness.interactions).toContainEqual({
      type: 'begin_transform',
      elements: [element.id],
    })

    fireEvent.pointerMove(harness.surface, { pointerId: 7, clientX: 55, clientY: 65 })
    expect(harness.interactions).toContainEqual({
      type: 'update_transform',
      frame: 1,
      elements: [
        {
          element: element.id,
          frame: { x: 35, y: 45, width: 100, height: 50, angle_degrees: 0 },
        },
      ],
    })

    fireEvent.pointerUp(harness.surface, { pointerId: 7, clientX: 58, clientY: 70 })
    expect(
      harness.interactions.filter((interaction) => interaction.type === 'update_transform').at(-1),
    ).toEqual({
      type: 'update_transform',
      frame: 2,
      elements: [
        {
          element: element.id,
          frame: { x: 38, y: 50, width: 100, height: 50, angle_degrees: 0 },
        },
      ],
    })
    expect(harness.commands).toEqual([{ type: 'finish_transform' }])
    expect(
      harness.interactions.some((interaction) => interaction.type === 'cancel_transform'),
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
    fireEvent.pointerCancel(harness.surface, { pointerId: 9 })

    expect(harness.interactions).toContainEqual({ type: 'cancel_transform' })
    expect(harness.commands).toEqual([])
  })
})
