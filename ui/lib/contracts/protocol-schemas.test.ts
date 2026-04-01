import { describe, expect, it } from 'vitest'
import {
  documentChangedEventSchema,
  downloadStateSchema,
  snapshotEventSchema,
} from './protocol-schemas'

describe('protocol schemas', () => {
  it('parses valid runtime snapshots', () => {
    const payload = {
      documents: [
        {
          documentUrl: '/documents/doc-1',
          hasBrushLayer: false,
          hasInpainted: true,
          hasRendered: false,
          hasSegment: true,
          height: 1400,
          id: 'doc-1',
          name: 'doc-1.png',
          revision: 2,
          textBlockCount: 4,
          thumbnailUrl: '/thumbnails/doc-1',
          width: 1000,
        },
      ],
      llm: {
        status: 'ready',
        modelId: 'openai:gpt-4.1',
        source: 'openai',
        error: null,
      },
      jobs: [
        {
          currentDocument: 0,
          currentStepIndex: 1,
          id: 'job-1',
          kind: 'pipeline',
          overallPercent: 25,
          status: 'running',
          step: 'ocr',
          totalDocuments: 1,
          totalSteps: 4,
        },
      ],
      downloads: [
        {
          id: 'download-1',
          filename: 'model.gguf',
          downloaded: 50,
          total: 100,
          status: 'downloading',
          error: null,
        },
      ],
    }

    expect(snapshotEventSchema.parse(payload)).toMatchObject(payload)
  })

  it('rejects malformed event payloads', () => {
    expect(
      downloadStateSchema.safeParse({
        id: 'download-1',
        filename: 'model.gguf',
        downloaded: 50,
        total: 100,
        status: 'paused',
        error: null,
      }).success,
    ).toBe(false)

    expect(
      documentChangedEventSchema.safeParse({
        documentId: 'doc-1',
        revision: -1,
        changed: [],
      }).success,
    ).toBe(false)
  })
})
