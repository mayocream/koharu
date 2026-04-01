import { describe, expect, it } from 'vitest'
import type { DocumentSummary } from '@/lib/contracts/protocol'
import { findDocumentIndex, resolveCurrentDocumentId } from './selection'

const createDocumentSummary = (id: string): DocumentSummary => ({
  documentUrl: `/documents/${id}`,
  hasBrushLayer: false,
  hasInpainted: false,
  hasRendered: false,
  hasSegment: false,
  height: 1400,
  id,
  name: `${id}.png`,
  revision: 1,
  textBlockCount: 0,
  thumbnailUrl: `/thumbnails/${id}`,
  width: 1000,
})

describe('document selection', () => {
  it('preserves the current document when it still exists', () => {
    const documents = [
      createDocumentSummary('doc-1'),
      createDocumentSummary('doc-2'),
    ]

    expect(resolveCurrentDocumentId(documents, 'doc-2')).toBe('doc-2')
  })

  it('falls back to the first document when the current selection is missing', () => {
    const documents = [
      createDocumentSummary('doc-1'),
      createDocumentSummary('doc-2'),
    ]

    expect(resolveCurrentDocumentId(documents, 'doc-99')).toBe('doc-1')
    expect(resolveCurrentDocumentId([], 'doc-99')).toBeUndefined()
  })

  it('finds the current document index', () => {
    const documents = [
      createDocumentSummary('doc-1'),
      createDocumentSummary('doc-2'),
    ]

    expect(findDocumentIndex(documents, 'doc-2')).toBe(1)
    expect(findDocumentIndex(documents, 'doc-99')).toBe(-1)
  })
})
