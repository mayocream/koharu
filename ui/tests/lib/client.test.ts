import { describe, expect, it } from 'vitest'

import { KoharuClient, useEditorStore } from '@/lib/koharu'
import { FakeBridge } from '@/tests/helpers'

function connected() {
  const bridge = new FakeBridge()
  Object.defineProperty(window, 'koharu', { value: bridge, configurable: true, writable: true })
  const client = new KoharuClient()
  client.connect()
  const synchronize = bridge.commands()[0]
  bridge.emit({ type: 'accepted', id: synchronize.id, revision: 3 })
  return { bridge, client }
}

describe('KoharuClient', () => {
  it('synchronizes on connection and serializes durable commands at accepted revisions', async () => {
    const { bridge, client } = connected()
    expect(bridge.sent[0]).toMatchObject({ type: 'ready' })
    expect(bridge.commands()[0].command).toEqual({ type: 'synchronize' })

    const first = client.command({ type: 'undo' })
    const second = client.command({ type: 'redo' })
    expect(bridge.commands()).toHaveLength(2)
    const firstMessage = bridge.commands()[1]
    expect(firstMessage.base).toBe(3)
    bridge.emit({ type: 'accepted', id: firstMessage.id, revision: 4 })
    await expect(first).resolves.toBe('accepted')

    const secondMessage = bridge.commands()[2]
    expect(secondMessage.base).toBe(4)
    bridge.emit({ type: 'accepted', id: secondMessage.id, revision: 5 })
    await expect(second).resolves.toBe('accepted')
    client.disconnect()
  })

  it('treats native dialog cancellation as a normal result', async () => {
    const { bridge, client } = connected()
    const pending = client.command({ type: 'open_project' })
    const message = bridge.commands()[1]
    bridge.emit({ type: 'command_cancelled', id: message.id, revision: 3 })
    await expect(pending).resolves.toBe('cancelled')
    expect(useEditorStore.getState().error).toBeNull()
    client.disconnect()
  })

  it('drops pending edits and synchronizes after a revision gap', async () => {
    const { bridge, client } = connected()
    useEditorStore.setState({
      revision: 3,
      project: {
        id: 'project',
        name: 'Book',
        visible_page: null,
        can_undo: false,
        can_redo: false,
      },
    })
    const pending = client.command({ type: 'undo' })
    bridge.emit({
      type: 'project_changed',
      from: 2,
      revision: 4,
      name: 'Remote',
      page_order: [],
      pages: [],
      deleted_pages: [],
      visible_page: null,
      can_undo: false,
      can_redo: false,
    })
    await expect(pending).rejects.toThrow('missed a project revision')
    expect(bridge.commands().at(-1)?.command).toEqual({ type: 'synchronize' })
    client.disconnect()
  })

  it('ignores malformed native events without throwing', () => {
    const { bridge, client } = connected()
    bridge.emit({ type: 'accepted', revision: 'wrong' })
    expect(useEditorStore.getState().error).toContain('malformed')
    client.disconnect()
  })

  it('accepts typed download progress from the native bridge', () => {
    useEditorStore.setState({ downloads: {} })
    const { bridge, client } = connected()
    bridge.emit({
      type: 'download_changed',
      state: 'running',
      id: 7,
      name: 'model.bin',
      completed: 25,
      total: 100,
    })
    expect(useEditorStore.getState().downloads[7]).toMatchObject({
      state: 'running',
      completed: 25,
      total: 100,
    })

    bridge.emit({ type: 'download_changed', state: 'finished', id: 7 })
    expect(useEditorStore.getState().downloads[7]).toBeUndefined()
    client.disconnect()
  })

  it('accepts settings with one model per phase', () => {
    useEditorStore.setState({ error: null, settings: null })
    const { bridge, client } = connected()
    bridge.emit({
      type: 'settings_changed',
      settings: {
        pipeline: {
          detection: { model: 'comic_text_detector' },
          segmentation: {
            model: 'speech_bubble_yolov8m',
            confidence: null,
            nms_iou: null,
          },
          ocr: { model: 'paddleocr_vl_1.6' },
          typography: { model: 'font_detector', top_k: 3 },
          inpainting: { model: 'lama' },
        },
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
        local_translation_models: ['lfm2.5-1.2b-instruct'],
        target_languages: [
          { tag: 'en-US', name: 'English' },
          { tag: 'ja-JP', name: 'Japanese' },
        ],
      },
    })

    expect(useEditorStore.getState().error).toBeNull()
    expect(useEditorStore.getState().settings?.translation.model.provider).toBe('local')
    client.disconnect()
  })
})
