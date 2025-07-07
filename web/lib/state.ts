import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'

type CanvasState = {
  image: Uint8Array | null
  setImage: (image: Uint8Array | null) => void
  scale: number
  setScale: (scale: number) => void
  texts: any[]
  setTexts: (blocks: any[]) => void
  segment: Uint8Array | null
  setSegment: (segment: Uint8Array) => void
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

type SettingsState = {
  openAIServer: string | null
  setOpenAIServer: (url: string) => void
  openAIToken: string | null
  setOpenAIToken: (token: string) => void
  openAIModel: string | null
  setOpenAIModel: (model: string) => void
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      openAIServer: null,
      setOpenAIServer: (url) => set({ openAIServer: url }),
      openAIToken: null,
      setOpenAIToken: (token) => set({ openAIToken: token }),
      openAIModel: null,
      setOpenAIModel: (model) => set({ openAIModel: model }),
    }),
    {
      name: 'settings-storage',
      storage: createJSONStorage(() => localStorage),
    }
  )
)
