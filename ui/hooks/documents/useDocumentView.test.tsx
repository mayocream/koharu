import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type {
  DocumentResource,
  DocumentSummary,
} from '@/lib/contracts/protocol'
import { getEditorUiState } from '@/hooks/ui/useEditorUiState'

const documentsApiMock = vi.hoisted(() => ({
  listDocuments: vi.fn(),
  getDocumentResource: vi.fn(),
  getDocumentThumbnail: vi.fn(),
}))

vi.mock('@/lib/infra/documents/api', () => ({
  listDocuments: documentsApiMock.listDocuments,
  getDocumentResource: documentsApiMock.getDocumentResource,
  getDocumentThumbnail: documentsApiMock.getDocumentThumbnail,
}))

import { useDocumentView } from './useDocumentView'

const createQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  })

const createWrapper = (queryClient: QueryClient) =>
  function QueryWrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )
  }

const createDocumentSummary = (id: string): DocumentSummary => ({
  documentUrl: `https://example.com/documents/${id}`,
  hasBrushLayer: false,
  hasInpainted: false,
  hasRendered: false,
  hasSegment: false,
  height: 1400,
  id,
  name: `${id}.png`,
  revision: 1,
  textBlockCount: 0,
  thumbnailUrl: `https://example.com/thumbnails/${id}`,
  width: 1000,
})

const createDocumentResource = (id: string): DocumentResource => ({
  id,
  path: `/tmp/${id}.png`,
  name: `${id}.png`,
  width: 1000,
  height: 1400,
  revision: 1,
  assets: {
    image: `https://example.com/${id}.png`,
    thumbnail: `https://example.com/${id}-thumb.png`,
    segment: null,
    inpainted: null,
    brushLayer: null,
    rendered: null,
  },
  textBlocks: [],
})

describe('useDocumentView', () => {
  beforeEach(() => {
    documentsApiMock.listDocuments.mockReset()
    documentsApiMock.getDocumentResource.mockReset()
    documentsApiMock.getDocumentThumbnail.mockReset()
    const editor = getEditorUiState()
    editor.setCurrentDocumentId(undefined)
    editor.setTotalPages(0)
  })

  it('selects the first document when there is no current selection', async () => {
    documentsApiMock.listDocuments.mockResolvedValue([
      createDocumentSummary('doc-1'),
    ])
    documentsApiMock.getDocumentResource.mockResolvedValue(
      createDocumentResource('doc-1'),
    )
    const queryClient = createQueryClient()

    const { result } = renderHook(() => useDocumentView(), {
      wrapper: createWrapper(queryClient),
    })

    await waitFor(() => {
      expect(result.current.currentDocumentId).toBe('doc-1')
      expect(result.current.currentDocument?.id).toBe('doc-1')
    })

    expect(getEditorUiState().currentDocumentId).toBe('doc-1')
    expect(documentsApiMock.listDocuments).toHaveBeenCalledTimes(1)
    expect(documentsApiMock.getDocumentResource).toHaveBeenCalledWith('doc-1')
  })

  it('preserves an existing current document selection when it still exists', async () => {
    getEditorUiState().setCurrentDocumentId('doc-2')
    documentsApiMock.listDocuments.mockResolvedValue([
      createDocumentSummary('doc-1'),
      createDocumentSummary('doc-2'),
    ])
    documentsApiMock.getDocumentResource.mockResolvedValue(
      createDocumentResource('doc-2'),
    )
    const queryClient = createQueryClient()

    const { result } = renderHook(() => useDocumentView(), {
      wrapper: createWrapper(queryClient),
    })

    await waitFor(() => {
      expect(result.current.currentDocumentId).toBe('doc-2')
      expect(result.current.currentDocument?.id).toBe('doc-2')
    })

    expect(getEditorUiState().currentDocumentId).toBe('doc-2')
    expect(documentsApiMock.getDocumentResource).toHaveBeenCalledWith('doc-2')
  })
})
