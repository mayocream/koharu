import { fireEvent, render, screen } from '@testing-library/react'
import { ThemeProvider } from 'next-themes'
import { describe, expect, it, vi } from 'vitest'

import { ActivityBubble } from '@/components/ActivityBubble'
import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { ToolRail } from '@/components/canvas/ToolRail'
import { MenuBar } from '@/components/MenuBar'
import { Navigator } from '@/components/Navigator'
import { Panels } from '@/components/Panels'
import { SettingsDialog } from '@/components/SettingsDialog'
import { TooltipProvider } from '@/components/ui/tooltip'
import { koharuClient, useEditorStore, type EntityView, type SettingsView } from '@/lib/koharu'

const emptyCredential = () => ({ configured: false, value: null, clear: false })

const textElement: EntityView = {
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
  image: null,
  source_text: { text: 'こんにちは', language: 'ja' },
  translation: { locale: 'en-US', text: 'Hello' },
  typography: {
    preferred_font: 'Noto Sans',
    size: 16,
    alignment: 'Center',
    writing_mode: 'Horizontal',
  },
  region: null,
}

const fontSettings: SettingsView = {
  pipeline: {
    detection: { model: 'koharu-layout-rfdetr-seg-2xl' },
    ocr: { model: 'paddleocr-vl-1.6' },
    typography: { model: 'font-detector', top_k: 3 },
    inpainting: { model: 'lama' },
  },
  translation: {
    model: { provider: 'local', model: 'lfm2.5-1.2b-instruct' },
    target_language: 'en-US',
    instructions: null,
    credentials: {
      openai: emptyCredential(),
      gemini: emptyCredential(),
      claude: emptyCredential(),
      deepseek: emptyCredential(),
      openai_compatible: emptyCredential(),
      openrouter: emptyCredential(),
      lm_studio: emptyCredential(),
      deepl: emptyCredential(),
      google_cloud_translation: emptyCredential(),
      caiyun: emptyCredential(),
    },
  },
  local_translation_models: [],
  target_languages: [],
  fonts: [
    {
      family_name: 'Noto Sans',
      post_script_name: 'NotoSans-Regular',
      weight: 400,
      stretch: 100,
      style: 'normal',
      source: 'system',
    },
    {
      family_name: 'Noto Sans',
      post_script_name: 'NotoSans-Bold',
      weight: 700,
      stretch: 100,
      style: 'normal',
      source: 'system',
    },
  ],
}

function installProject() {
  useEditorStore.setState({
    connection: 'connected',
    revision: 1,
    project: {
      id: 'project',
      name: 'Book',
      visible_page: 'page',
      can_undo: true,
      can_redo: false,
    },
    pages: [
      {
        id: 'page',
        label: 'Page 1',
        size: { width: 1000, height: 1500 },
        source: 'source',
        clean: 'clean',
        entities: 1,
      },
    ],
    page: {
      id: 'page',
      label: 'Page 1',
      size: { width: 1000, height: 1500 },
      assets: {
        source: 'source',
        clean: 'clean',
        rendered: null,
        text_mask: null,
        coo_mask: null,
        bubble_mask: null,
        brush_mask: null,
      },
      entities: [textElement],
    },
    selectedPages: ['page'],
    selectedElements: ['element'],
  })
}

describe('native editor components', () => {
  it('renders the localized project menu and custom-protocol navigator thumbnail', () => {
    installProject()
    const { container } = render(
      <>
        <MenuBar />
        <Navigator />
      </>,
    )
    expect(screen.queryByText('Book')).not.toBeInTheDocument()
    expect(screen.getByText('File')).toBeInTheDocument()
    expect(
      [...container.querySelectorAll('img')].find((image) =>
        image.getAttribute('src')?.startsWith('koharu-resource:'),
      ),
    ).toHaveAttribute('src', 'koharu-resource://project/project/blob/clean?width=320')
  })

  it('starts native resizing from each frameless window edge and corner', () => {
    installProject()
    const resize = vi.spyOn(koharuClient, 'controlWindow').mockImplementation(() => undefined)
    render(<MenuBar />)

    const actions = [
      'resize-north',
      'resize-east',
      'resize-south',
      'resize-west',
      'resize-north-east',
      'resize-south-east',
      'resize-south-west',
      'resize-north-west',
    ]
    for (const action of actions) {
      fireEvent.pointerDown(screen.getByTestId(`window-${action}`), { button: 0 })
    }

    expect(resize.mock.calls.map(([action]) => action)).toEqual(
      actions.map((action) => action.replaceAll('-', '_')),
    )
    fireEvent.pointerDown(screen.getByTestId('window-resize-east'), { button: 2 })
    expect(resize).toHaveBeenCalledTimes(actions.length)

    fireEvent.click(screen.getByRole('button', { name: 'Maximize' }))
    expect(screen.queryByTestId('window-resize-east')).not.toBeInTheDocument()
  })

  it('switches canvas tools and restores the compact render controls', () => {
    installProject()
    useEditorStore.setState({ settings: fontSettings })
    render(
      <TooltipProvider>
        <CanvasToolbar />
        <ToolRail />
        <Panels />
      </TooltipProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Brush mask' }))
    expect(useEditorStore.getState().tool).toBe('brush_mask')
    expect(screen.queryByLabelText('Page view')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Fit Window' })).not.toBeInTheDocument()
    expect(screen.getByTestId('layer-textBlocks')).toHaveAttribute('data-visible', 'true')
    expect(screen.queryByTestId('layer-rendered')).not.toBeInTheDocument()
    expect(screen.getByTestId('textblocks-count')).toHaveAttribute('data-count', '1')
    expect(screen.getByTestId('textblock-card-0')).toHaveAttribute('data-selected', 'true')
    expect(screen.getByTestId('textblock-translation-element')).toHaveValue('Hello')
    const renderTab = screen.getByRole('tab', { name: /render/i })
    fireEvent.mouseDown(renderTab, { button: 0, ctrlKey: false })
    expect(renderTab).toHaveAttribute('data-state', 'active')
    expect(screen.getByTestId('render-controls-panel')).toBeInTheDocument()
    expect(screen.getByTestId('render-font-select')).toHaveTextContent('Noto Sans')
    expect(screen.getByTestId('render-font-size')).toHaveValue(16)
    expect(screen.getByTestId('render-align-center')).toHaveAttribute('data-variant', 'toggle_on')
  })

  it('shows retained job progress and the typed settings builder', () => {
    useEditorStore.setState({
      jobs: {
        job: {
          state: 'running',
          id: 'job',
          kind: 'pipeline',
          completed: 1,
          total: 4,
          phase: 'ocr',
          model: 'manga-ocr',
        },
      },
      settingsOpen: true,
      settings: {
        pipeline: {
          detection: { model: 'koharu-layout-rfdetr-seg-2xl' },
          ocr: { model: 'paddleocr-vl-1.6' },
          typography: { model: 'font-detector', top_k: 3 },
          inpainting: { model: 'lama' },
        },
        translation: {
          model: {
            provider: 'openai',
            model: 'gpt-4.1-mini',
            temperature: null,
            max_tokens: null,
            thinking: false,
          },
          target_language: 'en-US',
          instructions: null,
          credentials: {
            openai: { configured: true, value: null, clear: false },
            gemini: emptyCredential(),
            claude: emptyCredential(),
            deepseek: emptyCredential(),
            openai_compatible: emptyCredential(),
            openrouter: emptyCredential(),
            lm_studio: emptyCredential(),
            deepl: emptyCredential(),
            google_cloud_translation: emptyCredential(),
            caiyun: emptyCredential(),
          },
        },
        local_translation_models: ['lfm2.5-1.2b-instruct'],
        target_languages: [
          { tag: 'en-US', name: 'English' },
          { tag: 'ja-JP', name: 'Japanese' },
        ],
        fonts: [],
      },
    })
    render(
      <ThemeProvider attribute='class'>
        <ActivityBubble />
        <SettingsDialog />
      </ThemeProvider>,
    )
    expect(screen.getByText('25%')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /pipeline/i }))
    expect(screen.getByText('Pipeline', { selector: 'h3' })).toBeInTheDocument()
    expect(
      screen.queryByText('Settings are unavailable while disconnected.'),
    ).not.toBeInTheDocument()
    expect(screen.getAllByRole('combobox').length).toBeGreaterThanOrEqual(5)
    expect(screen.queryByText('Segmentation')).not.toBeInTheDocument()
    expect(screen.getByText(/^typography$/i)).toBeInTheDocument()
    expect(screen.getByLabelText('Top predictions')).toHaveValue(3)
    const credential = screen.getByLabelText('openai credential')
    expect(credential).toHaveAttribute('type', 'password')
    expect(credential).toHaveValue('')
    expect(credential).toHaveAttribute('placeholder', 'Configured')
    fireEvent.click(screen.getByRole('button', { name: 'Reveal credential' }))
    expect(credential).toHaveAttribute('type', 'password')
  })

  it('shows runtime download progress', () => {
    useEditorStore.setState({
      jobs: {},
      downloads: {
        7: {
          state: 'running',
          id: 7,
          name: 'model.bin',
          completed: 50,
          total: 100,
        },
      },
    })
    render(<ActivityBubble />)
    expect(screen.getByText('Download')).toBeInTheDocument()
    expect(screen.getByText('model.bin')).toBeInTheDocument()
    expect(screen.getByText('50%')).toBeInTheDocument()
    expect(screen.getByText('50 B / 100 B')).toBeInTheDocument()
  })
})
