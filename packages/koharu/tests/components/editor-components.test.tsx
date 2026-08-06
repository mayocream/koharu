import { QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render as testingRender, screen, waitFor } from '@testing-library/react'
import { ThemeProvider } from 'next-themes'
import type { ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { ActivityCenter } from '@/components/editor/ActivityCenter'
import { CanvasCommandBar } from '@/components/editor/CanvasCommandBar'
import { Inspector } from '@/components/editor/Inspector'
import { PageRail } from '@/components/editor/PageRail'
import { ResourceMonitor } from '@/components/editor/ResourceMonitor'
import { StatusBar } from '@/components/editor/StatusBar'
import { ToolBar } from '@/components/editor/ToolBar'
import { SettingsPage } from '@/components/preferences/SettingsPage'
import { commands, type Layer, type Preferences } from '@/lib/protocol'
import { pageKey, pagesKey, projectKey, queryClient } from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { TooltipProvider } from '@koharu/ui/components/tooltip'

const nativeWindow = vi.hoisted(() => ({
  close: vi.fn(async () => undefined),
  minimize: vi.fn(async () => undefined),
  toggleMaximize: vi.fn(async () => undefined),
}))

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => nativeWindow }))

const emptyCredential = () => ({ configured: false, value: null, clear: false })

const textLayer: Layer = {
  type: 'text',
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
  content: {
    id: 'content',
    source: { text: 'こんにちは', language: 'ja' },
    translation: { text: 'Hello', language: null },
    role: null,
    source_region: null,
  },
  typography: {
    preferred_font: 'Noto Sans',
    font_weight: 400,
    size: null,
    auto_fit: true,
    color: [0, 0, 0, 255],
    stroke_color: [255, 255, 255, 255],
    stroke_width: 0,
    alignment: 'Center',
    writing_mode: 'Horizontal',
  },
  layout: 'paragraph',
  fit_region: null,
}

const preferences: Preferences = {
  pipeline: {
    detection: { model: 'koharu-layout-rfdetr-seg-2xl' },
    ocr: { model: 'paddleocr-vl-1.6' },
    translation: {
      model: {
        provider: 'local',
        model: 'lfm2.5-1.2b-instruct',
        quantization: null,
      },
      generation: {},
      target_language: 'en-US',
      instructions: null,
    },
    inpainting: { model: 'lama' },
    processor: {},
  },
  providers: {
    entries: [
      {
        name: 'Local',
        config: { provider: 'local', settings: {} },
        credential: null,
      },
      {
        name: 'OpenAI-compatible',
        config: {
          provider: 'openai-compatible',
          settings: { base_url: 'http://localhost:11434/v1' },
        },
        credential: emptyCredential(),
      },
      {
        name: 'LM Studio',
        config: { provider: 'lm-studio', settings: { base_url: 'http://localhost:1234' } },
        credential: emptyCredential(),
      },
      {
        name: 'DeepL',
        config: { provider: 'deepl', settings: { base_url: null } },
        credential: emptyCredential(),
      },
    ],
  },
  languages: [
    { tag: 'en-US', name: 'English' },
    { tag: 'ja-JP', name: 'Japanese' },
  ],
}

function installProject() {
  const page = {
    id: 'page',
    label: 'Page 1',
    size: { width: 1000, height: 1500 },
    assets: {
      source: 'source',
      rendered: null,
      text_mask: null,
      coo_mask: null,
      bubble_mask: null,
    },
    layers: [textLayer],
    regions: [],
  }
  queryClient.setQueryData(projectKey, {
    name: 'Book',
    revision: 1,
    active_page: 'page',
    can_undo: true,
    can_redo: false,
  })
  queryClient.setQueryData(pagesKey, [
    {
      id: 'page',
      label: 'Page 1',
      size: { width: 1000, height: 1500 },
      source_asset: 'source',
      layer_count: 1,
    },
  ])
  queryClient.setQueryData(pageKey, page)
  useKoharuStore.setState({
    preferences,
    translationModels: [
      {
        provider: 'local',
        model: 'lfm2.5-1.2b-instruct',
        name: 'LFM 2.5 1.2B Instruct',
        quantizations: [],
      },
    ],
    selectedPages: ['page'],
    selectedLayers: ['element'],
    layerFrames: {
      element: { x: 10, y: 20, width: 100, height: 50, angle_degrees: 0 },
    },
  })
  vi.spyOn(commands, 'getTranslationModels').mockImplementation(async () => [
    ...useKoharuStore.getState().translationModels,
  ])
}

function render(ui: ReactNode) {
  return testingRender(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>)
}

describe('greenfield editor', () => {
  it('shows import activity and prevents duplicate imports', async () => {
    installProject()
    let finishImport: (() => void) | undefined
    const importPages = vi.spyOn(commands, 'importPages').mockImplementation(
      () =>
        new Promise<null>((resolve) => {
          finishImport = () => resolve(null)
        }),
    )
    render(<PageRail />)

    const trigger = screen.getByRole('button', { name: 'Import pages' })
    fireEvent.click(trigger)
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Files...' }))

    expect(await screen.findByRole('status')).toHaveTextContent('Importing pages…')
    expect(trigger).toBeDisabled()
    expect(importPages).toHaveBeenCalledTimes(1)

    finishImport?.()
    await waitFor(() => expect(screen.queryByRole('status')).not.toBeInTheDocument())
  })

  it('loads page thumbnails into the filmstrip', async () => {
    installProject()
    const thumbnail = vi.spyOn(commands, 'getThumbnail').mockResolvedValue([1])
    render(<PageRail />)
    await waitFor(() => expect(thumbnail).toHaveBeenCalledWith('page'))
    expect(await screen.findByRole('img', { name: 'Page 1' })).toHaveAttribute(
      'src',
      'blob:koharu-thumbnail',
    )
    expect(screen.queryByText('01')).not.toBeInTheDocument()
  })

  it('switches tools and applies typography from the contextual inspector', async () => {
    installProject()
    const setTypography = vi.spyOn(commands, 'setTypography').mockResolvedValue(null)
    render(
      <TooltipProvider>
        <ToolBar />
        <Inspector />
      </TooltipProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Brush' }))
    expect(useKoharuStore.getState().tool).toBe('draw')
    fireEvent.click(screen.getByRole('button', { name: 'Text' }))
    expect(screen.getByTestId('type-inspector')).toBeInTheDocument()
    expect(screen.getByTestId('type-font-picker')).toHaveTextContent('Noto Sans')
    expect(screen.getByTestId('type-size')).toHaveValue('0')
    fireEvent.change(screen.getByTestId('type-size'), { target: { value: '18' } })
    await waitFor(() =>
      expect(setTypography).toHaveBeenCalledWith(
        expect.arrayContaining([
          expect.objectContaining({
            layer: 'element',
            typography: expect.objectContaining({ size: 18 }),
          }),
        ]),
      ),
    )
  })

  it('debounces layer text editing and flushes it when focus leaves the field', async () => {
    installProject()
    const save = vi.spyOn(commands, 'setSourceText').mockResolvedValue(null)
    render(<Inspector />)
    const layer = screen.getByRole('button', { name: 'Edit Hello' })
    expect(screen.getByTestId('edit-source-element')).toBeInTheDocument()
    fireEvent.click(layer)
    expect(screen.queryByTestId('edit-source-element')).not.toBeInTheDocument()
    fireEvent.click(layer)
    const source = screen.getByTestId('edit-source-element')
    fireEvent.change(source, { target: { value: 'corrected OCR' } })
    fireEvent.blur(source)
    await waitFor(() => expect(save).toHaveBeenCalledWith('element', 'corrected OCR'))
  })

  it('shows actual layers with only the useful text-role distinction', () => {
    installProject()
    queryClient.setQueryData(pageKey, (page: { layers: Layer[] }) => ({
      ...page,
      layers: [
        ...page.layers.map((layer) =>
          layer.type === 'text'
            ? { ...layer, content: { ...layer.content, role: 'dev.koharu.text.onomatopoeia' } }
            : layer,
        ),
        {
          ...textLayer,
          id: 'dialogue',
          content: {
            ...textLayer.content,
            id: 'dialogue-content',
            translation: { text: 'Dialogue line', language: null },
            role: 'dev.koharu.text.dialogue',
          },
        },
        {
          ...textLayer,
          id: 'free-text',
          content: {
            ...textLayer.content,
            id: 'free-text-content',
            translation: { text: 'Caption', language: null },
            role: 'dev.koharu.text.free-text',
          },
        },
      ],
    }))
    render(<Inspector />)

    expect(screen.queryByRole('button', { name: /Filter layers by type/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Edit Hello' })).toHaveTextContent('text')
    expect(screen.getByRole('button', { name: 'Edit Dialogue line' })).toHaveTextContent('dialogue')
    expect(screen.getByRole('button', { name: 'Edit Caption' })).toHaveTextContent('free-text')
    expect(screen.queryByText('Onomatopoeia')).not.toBeInTheDocument()
  })

  it('resets a custom text frame to its automatic fit region', async () => {
    installProject()
    queryClient.setQueryData(pageKey, (page: { layers: Layer[] }) => ({
      ...page,
      layers: page.layers.map((layer) =>
        layer.type === 'text' ? { ...layer, fit_region: 'bubble' } : layer,
      ),
    }))
    const reset = vi.spyOn(commands, 'setGeometry').mockResolvedValue(null)
    render(<Inspector />)

    expect(screen.getByText('Custom frame')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Reset to auto fit' }))

    await waitFor(() => expect(reset).toHaveBeenCalledWith([{ layer: 'element', points: null }]))
  })

  it('shows zoom before page size without a fit control', () => {
    installProject()
    useKoharuStore.setState({ camera: { zoom: 1.25, translation: [0, 0], fitted: false } })
    render(<StatusBar />)

    const zoom = screen.getByText('125%')
    const size = screen.getByText('1000 × 1500 px')
    expect(zoom.compareDocumentPosition(size) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0)
    expect(screen.queryByRole('button', { name: 'Fit window' })).not.toBeInTheDocument()
  })

  it('changes the pipeline scope and selected stages from the runtime selector', async () => {
    installProject()
    const run = vi.spyOn(commands, 'process').mockResolvedValue('job')
    render(<CanvasCommandBar />)

    fireEvent.click(screen.getByRole('button', { name: 'AI runtime selector' }))
    fireEvent.click(screen.getByRole('button', { name: /Scope Page/ }))
    fireEvent.click(screen.getByRole('button', { name: /Entire project/ }))
    fireEvent.click(screen.getByRole('button', { name: /Stages 4 stages/ }))
    fireEvent.click(screen.getByRole('button', { name: /Translation/ }))
    fireEvent.click(screen.getByRole('button', { name: /Inpainting/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Run AI processing' }))
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith(
        { scope: 'project' },
        { operation: 'stages', stages: ['detection', 'ocr'] },
      ),
    )

    fireEvent.click(screen.getByRole('button', { name: 'AI runtime selector' }))
    fireEvent.click(screen.getByRole('button', { name: /Scope Project/ }))
    fireEvent.click(screen.getByRole('button', { name: /Selected pages/ }))
    fireEvent.click(screen.getByRole('button', { name: /Stages 2 stages/ }))
    fireEvent.click(screen.getByRole('button', { name: /Translation/ }))
    fireEvent.click(screen.getByRole('button', { name: /Inpainting/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Run AI processing' }))
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith(
        { scope: 'pages', value: ['page'] },
        { operation: 'full' },
      ),
    )
  })

  it('runs the current page and exposes the runtime shortcuts', async () => {
    installProject()
    const run = vi.spyOn(commands, 'process').mockResolvedValue('job')
    render(<CanvasCommandBar />)

    fireEvent.click(screen.getByRole('button', { name: 'Run AI processing' }))
    await waitFor(() =>
      expect(run).toHaveBeenLastCalledWith(
        { scope: 'pages', value: ['page'] },
        { operation: 'full' },
      ),
    )

    fireEvent.click(screen.getByRole('button', { name: 'AI runtime selector' }))
    expect(screen.getByRole('button', { name: /Model LFM 2.5 1.2B Instruct/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Scope Page/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Stages 4 stages/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(useKoharuStore.getState().settingsOpen).toBe(true)
  })

  it('changes the translation model from the runtime selector', async () => {
    installProject()
    const nextPreferences: Preferences = {
      ...preferences,
      pipeline: {
        ...preferences.pipeline,
        translation: {
          ...preferences.pipeline.translation,
          model: { provider: 'local', model: 'gemma4-12b-it', quantization: null },
        },
      },
    }
    const save = vi.spyOn(commands, 'savePreferences').mockResolvedValue(nextPreferences)
    useKoharuStore.setState({
      translationModels: [
        ...useKoharuStore.getState().translationModels,
        {
          provider: 'local',
          model: 'gemma4-12b-it',
          name: 'Gemma 4 12B',
          quantizations: [],
        },
      ],
    })
    render(<CanvasCommandBar />)

    fireEvent.click(screen.getByRole('button', { name: 'AI runtime selector' }))
    fireEvent.click(screen.getByRole('button', { name: /Model LFM 2.5 1.2B Instruct/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Use Gemma 4 12B from Local' }))

    await waitFor(() =>
      expect(save).toHaveBeenCalledWith(nextPreferences.pipeline, preferences.providers),
    )
    expect(useKoharuStore.getState().preferences?.pipeline.translation.model).toEqual(
      nextPreferences.pipeline.translation.model,
    )
  })

  it('edits pipeline and translation preferences from the settings page', () => {
    installProject()
    useKoharuStore.setState({ settingsOpen: true })
    render(
      <ThemeProvider attribute='class'>
        <SettingsPage />
      </ThemeProvider>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Pipeline' }))
    expect(screen.getByRole('heading', { level: 2, name: 'Pipeline' })).toBeInTheDocument()
    expect(screen.getAllByRole('combobox')).toHaveLength(3)
    fireEvent.click(screen.getByRole('button', { name: 'Providers' }))
    expect(screen.getByRole('heading', { level: 2, name: 'Providers' })).toBeInTheDocument()
    expect(screen.getByLabelText('DeepL credential')).toBeInTheDocument()
    expect(screen.getAllByLabelText('Base URL')).toHaveLength(3)
    fireEvent.click(screen.getByRole('button', { name: 'Translation' }))
    expect(screen.getByRole('heading', { level: 2, name: 'Translation' })).toBeInTheDocument()
    expect(screen.getByLabelText('Translation model')).toHaveTextContent('lfm2.5-1.2b-instruct')
    expect(screen.getByLabelText('Target language')).toHaveTextContent('American English')
  })

  it('shows Koharu model resources in the left sidebar footer', () => {
    useKoharuStore.setState({
      resources: {
        process_memory: 2 * 1024 ** 3,
        system_memory: 64 * 1024 ** 3,
        process_cpu: 8,
        devices: [
          {
            name: 'GPU',
            selected: true,
            memory_budget: 16 * 1024 ** 3,
            memory_used: 6 * 1024 ** 3,
            utilization: 40,
          },
        ],
      },
    })
    render(<ResourceMonitor />)
    expect(screen.getByText('8.0%')).toBeInTheDocument()
    expect(screen.getByText('3.1%')).toBeInTheDocument()
  })

  it('keeps running work visible and stoppable', async () => {
    installProject()
    useKoharuStore.setState({
      jobs: {
        job: {
          state: 'running',
          id: 'job',
          completed: 1,
          total: 4,
          page: 'page',
          stage: 'ocr',
          model: 'manga-ocr',
          error: null,
        },
      },
    })
    const stop = vi.spyOn(commands, 'stopJob').mockResolvedValue(null)
    render(<ActivityCenter />)
    expect(screen.getByText('25%')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    await waitFor(() => expect(stop).toHaveBeenCalledWith('job'))
  })
})
