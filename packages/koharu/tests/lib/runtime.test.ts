import { act, render, screen, waitFor } from '@testing-library/react'
import { createElement, StrictMode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import Providers from '@/app/providers'
import { call } from '@/lib/backend'
import { commands, type Preferences, type ProjectInfo, type StartupState } from '@/lib/protocol'
import { useProject } from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { MemoryTransport, RequestError, setTransport } from '@/lib/transport'

let transport: MemoryTransport

const preferences: Preferences = {
  pipeline: {
    detection: { model: 'koharu-layout-rfdetr-seg-2xl' },
    ocr: { model: 'paddleocr-vl-1.6' },
    translation: {
      model: {
        provider: 'local',
        model: 'lfm2.5-1.2b-instruct',
        quantization: null,
        vision: true,
      },
      generation: {},
      target_language: 'en-US',
      instructions: null,
    },
    inpainting: { model: 'lama' },
    processor: {},
  },
  providers: {
    entries: [],
  },
  typesetting: {
    font_families: ['CCWildWords', 'Adobe 黑体 Std'],
  },
  languages: [],
}

const project: ProjectInfo = {
  name: 'Book',
  revision: 3,
  active_page: null,
  can_undo: true,
  can_redo: false,
}

beforeEach(() => {
  transport = new MemoryTransport()
  setTransport(transport)
  vi.spyOn(commands, 'getStartup').mockRejectedValue(
    new RequestError({ code: 'not_ready', message: 'Koharu is still starting.' }),
  )
  vi.spyOn(commands, 'getTranslationModels').mockResolvedValue([])
})

const startupState = (): StartupState => ({
  preferences,
  jobs: [],
  canvas: {
    zoom: 1,
    translation: [0, 0],
    fitted: true,
    element_frames: [],
  },
})

async function start() {
  const view = render(createElement(Providers, null, createElement('div')))
  act(() => transport.emit({ type: 'startup_ready', startup: startupState() }))
  await waitFor(() => expect(useKoharuStore.getState().preferences).toBe(preferences))
  return { dispose: view.unmount }
}

function ProjectProbe() {
  const project = useProject().data
  return createElement(
    'span',
    null,
    project === undefined ? 'Loading' : (project?.name ?? 'Closed'),
  )
}

describe('runtime', () => {
  it('reconciles startup when the ready event was emitted before React subscribed', async () => {
    vi.spyOn(commands, 'getStartup').mockResolvedValue(startupState())

    const view = render(createElement(Providers, null, createElement('div')))

    await waitFor(() => expect(useKoharuStore.getState().startup.state).toBe('ready'))
    expect(useKoharuStore.getState().preferences).toBe(preferences)
    view.unmount()
  })

  it('keeps waiting after a not-ready startup snapshot and accepts the ready event', async () => {
    const view = render(createElement(Providers, null, createElement('div')))

    await waitFor(() => expect(commands.getStartup).toHaveBeenCalledOnce())
    expect(useKoharuStore.getState().startup.state).toBe('connecting')

    act(() => transport.emit({ type: 'startup_ready', startup: startupState() }))
    expect(useKoharuStore.getState().startup.state).toBe('ready')
    view.unmount()
  })

  it('surfaces startup snapshot failures other than not-ready', async () => {
    vi.spyOn(commands, 'getStartup').mockRejectedValue(
      new RequestError({ code: 'internal', message: 'Startup snapshot failed.' }),
    )

    const view = render(createElement(Providers, null, createElement('div')))

    await waitFor(() =>
      expect(useKoharuStore.getState().startup).toEqual({
        state: 'failed',
        error: { code: 'internal', message: 'Startup snapshot failed.' },
      }),
    )
    view.unmount()
  })

  it('keeps the project unresolved until its backend query returns', async () => {
    const projectPending = deferred<ProjectInfo | null>()
    vi.spyOn(commands, 'getProject').mockReturnValue(projectPending.promise)
    const view = render(createElement(Providers, null, createElement(ProjectProbe)))
    act(() => transport.emit({ type: 'startup_ready', startup: startupState() }))

    expect(await screen.findByText('Loading')).toBeInTheDocument()
    projectPending.resolve(null)
    expect(await screen.findByText('Closed')).toBeInTheDocument()
    view.unmount()
  })

  it('keeps one live event stream through Strict Mode effect replay', async () => {
    const view = render(
      createElement(StrictMode, null, createElement(Providers, null, createElement('div'))),
    )

    act(() => {
      transport.emit({ type: 'startup_ready', startup: startupState() })
      transport.emit({
        type: 'job',
        job: {
          id: 'job',
          state: 'running',
          completed: 0,
          total: 4,
          page: 'page',
          stage: 'detection',
          model: 'model',
          error: null,
        },
      })
    })

    expect(useKoharuStore.getState().jobs.job).toMatchObject({ state: 'running', total: 4 })
    view.unmount()
  })

  it('passes only domain arguments to mutation commands', async () => {
    const rename = vi.spyOn(commands, 'renamePage').mockResolvedValue(null)

    await expect(call(commands.renamePage, 'page', 'Chapter 1')).resolves.toBeNull()
    expect(rename).toHaveBeenCalledWith('page', 'Chapter 1')
  })

  it('passes the managed project name to open', async () => {
    const open = vi.spyOn(commands, 'openProject').mockResolvedValue(null)
    await expect(call(commands.openProject, 'Volume 1')).resolves.toBeNull()
    expect(open).toHaveBeenCalledWith('Volume 1')
  })

  it('applies independent event updates directly to the store', async () => {
    useKoharuStore.setState({ downloads: {}, resources: null })
    const { dispose } = await start()

    transport.emit({
      type: 'download',
      download: {
        id: 7,
        state: 'running',
        name: 'model.bin',
        completed: 25,
        total: 100,
        error: null,
      },
    })
    transport.emit({
      type: 'resources',
      resources: {
        process_memory: 1024,
        system_memory: 8192,
        process_cpu: 5,
        devices: [
          { name: 'GPU', selected: true, memory_budget: 8192, memory_used: 4096, utilization: 40 },
        ],
      },
    })
    expect(useKoharuStore.getState().downloads[7]).toMatchObject({ completed: 25, total: 100 })
    expect(useKoharuStore.getState().resources).toMatchObject({
      process_cpu: 5,
      devices: [{ memory_used: 4096 }],
    })
    dispose()
  })

  it('refreshes project queries when an autonomous job commits work', async () => {
    vi.spyOn(commands, 'getProject').mockResolvedValueOnce(null).mockResolvedValue(project)
    const view = render(createElement(Providers, null, createElement(ProjectProbe)))
    act(() => transport.emit({ type: 'startup_ready', startup: startupState() }))
    expect(await screen.findByText('Closed')).toBeInTheDocument()

    transport.emit({
      type: 'job',
      job: {
        id: 'job',
        state: 'running',
        completed: 1,
        total: 2,
        page: 'page',
        stage: 'ocr',
        model: 'model',
        error: null,
      },
    })

    expect(await screen.findByText('Book')).toBeInTheDocument()
    view.unmount()
  })
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}
