import { create } from 'zustand'

type CanvasState = {
  image: ImageBitmap | null
  setImage: (image: ImageBitmap | null) => void
  scale: number
  setScale: (scale: number) => void
  texts: any[]
  setTexts: (blocks: any[]) => void
  segment: ImageBitmap | null
  setSegment: (segment: ImageBitmap) => void
}

export const useCanvasStore = create<CanvasState>((set) => ({
  image: null,
  setImage: (image) => set({ image }),
  scale: 1,
  setScale: (scale) => set({ scale }),
  texts: [],
  setTexts: (blocks) => set({ texts: blocks }),
  segment: null,
  setSegment: (segment) => set({ segment }),
}))

type WorkflowState = {
  selectedTool: string
  setSelectedTool: (tool: string) => void
  prompt: string
  setPrompt: (prompt: string) => void
  selectedTextIndex: number | null
  setSelectedTextIndex: (index: number | null) => void
}

export const useWorkflowStore = create<WorkflowState>((set) => ({
  selectedTool: 'detection',
  setSelectedTool: (tool) => set({ selectedTool: tool }),
  prompt: '',
  setPrompt: (prompt) => set({ prompt }),
  selectedTextIndex: null,
  setSelectedTextIndex: (index) => set({ selectedTextIndex: index }),
}))
