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
import { koharuClient, useEditorStore, type Element, type SettingsView } from '@/lib/koharu'

const textElement: Element = {
  id: 'element',
  frame: { x: 10, y: 20, width: 100, height: 50, angle_degrees: 0 },
  visible: true,
  opacity: 1,
  kind: {
    Text: {
      source: { text: 'こんにちは', language: 'ja', direction: 'Auto', confidence: 0.9, lines: [] },
      translation: 'Hello',
      style: {
        font_families: ['Noto Sans'],
        font_size: 16,
        font_weight: 400,
        font_stretch: 100,
        font_slant: 'Normal',
        color: [0, 0, 0, 255],
        line_height: 1.2,
        letter_spacing: 0,
        word_spacing: 0,
        horizontal_scale: 100,
        vertical_scale: 100,
        baseline_shift: 0,
        angle_degrees: 0,
        decoration: { underline: false, strikethrough: false },
        effects: [],
      },
      layout: {
        horizontal_align: 'Center',
        vertical_align: 'Center',
        writing_mode: 'Auto',
        inset: [0, 0, 0, 0],
        overflow: 'Visible',
        fit: 'Bubble',
      },
      role: 'Dialogue',
      panel: null,
      bubble: null,
      reading_order: 0,
      polygon: [],
      predictions: [],
    },
  },
}

const fontSettings: SettingsView = {
  pipeline: { processors: [] },
  translation: {
    model: { provider: 'local', model: 'lfm2.5-1.2b-instruct' },
    target_language: 'en-US',
    instructions: null,
    credentials: {
      openai: '',
      gemini: '',
      claude: '',
      deepseek: '',
      openai_compatible: '',
      openrouter: '',
      lm_studio: '',
      deepl: '',
      google_cloud_translation: '',
      caiyun: '',
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
      category: null,
      cached: true,
    },
    {
      family_name: 'Noto Sans',
      post_script_name: 'NotoSans-Bold',
      weight: 700,
      stretch: 100,
      style: 'normal',
      source: 'system',
      category: null,
      cached: true,
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
        name: 'Page 1',
        size: { width: 1000, height: 1500 },
        source: 'source',
        clean: 'clean',
        elements: 1,
      },
    ],
    page: {
      id: 'page',
      name: 'Page 1',
      size: { width: 1000, height: 1500 },
      source: 'source',
      assets: {
        clean: 'clean',
        rendered: null,
        text_mask: null,
        coo_mask: null,
        bubble_mask: null,
        brush_mask: null,
      },
      elements: [textElement],
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
    expect(screen.getByTestId('render-font-variant-select')).toHaveTextContent(
      'render.fontWeights.regular',
    )
    expect(screen.getByTestId('render-font-size')).toHaveValue(16)
    expect(screen.getByTestId('render-effect-toggle-bold')).toHaveAttribute(
      'data-variant',
      'toggle_off',
    )
    expect(screen.getByTestId('render-align-center')).toHaveAttribute('data-variant', 'toggle_on')
    expect(screen.getByTestId('render-stroke-enable')).toHaveAttribute('data-variant', 'toggle_off')
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
          model: 'manga_ocr',
        },
      },
      settingsOpen: true,
      settings: {
        pipeline: {
          processors: [
            { model: 'comic_text_detector' },
            {
              model: 'comic_layout_yolo26s',
              confidence: 0.25,
              text_regions: false,
              text_masks: true,
            },
            {
              model: 'comic_onomatopoeia',
              detection_threshold: 0.5,
              recognition_threshold: 0.5,
              dedup_iou: 0.30000001192092896,
            },
            { model: 'speech_bubble_yolov8m', confidence: null, nms_iou: null },
            { model: 'paddleocr_vl_1.6' },
            { model: 'font_detector', top_k: 3 },
            { model: 'lama' },
          ],
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
            openai: 'secret-value',
            gemini: '',
            claude: '',
            deepseek: '',
            openai_compatible: '',
            openrouter: '',
            lm_studio: '',
            deepl: '',
            google_cloud_translation: '',
            caiyun: '',
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
    expect(screen.getByRole('switch', { name: 'Comic Layout YOLO26s' })).toBeChecked()
    expect(screen.getByLabelText('Dedup IoU')).toHaveValue(0.3)
    const credential = screen.getByLabelText('openai credential')
    expect(credential).toHaveAttribute('type', 'password')
    expect(credential).toHaveValue('secret-value')
    fireEvent.click(screen.getByRole('button', { name: 'Reveal credential' }))
    expect(credential).toHaveAttribute('type', 'text')
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
