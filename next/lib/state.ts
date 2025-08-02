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
  tool: string
  setTool: (tool: string) => void
}

export const useWorkflowStore = create<WorkflowState>((set) => ({
  tool: 'detection',
  setTool: (tool) => set({ tool: tool }),
}))
