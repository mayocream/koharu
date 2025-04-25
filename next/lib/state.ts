import { LazyStore } from '@tauri-apps/plugin-store'
import { create } from 'zustand'
import { createJSONStorage, persist, StateStorage } from 'zustand/middleware'

// refer: https://github.com/mrousavy/react-native-mmkv/blob/main/docs/WRAPPER_ZUSTAND_PERSIST_MIDDLEWARE.md
const createStorage = (name: string) => {
  const store = new LazyStore(name)
  return {
    getItem: async (name: string) => {
      const value = await store.get<string>(name)
      return value
    },
    setItem: async (name: string, value: string) => {
      await store.set(name, value)
    },
    removeItem: async (name: string) => {
      await store.delete(name)
    },
  } as StateStorage
}

type CanvasState = {
  imageSrc: string | null
  setImageSrc: (src: string | null) => void
  scale: number
  setScale: (scale: number) => void
  texts: any[]
  setTexts: (blocks: any[]) => void
  segment: Uint8Array | null
  setSegment: (segment: Uint8Array) => void
  imageSrcHistory: string[]
  setImageSrcHistory: (src: string) => void
}

export const useCanvasStore = create<CanvasState>()(
  persist(
    (set, get) => ({
      imageSrc: null,
      setImageSrc: (src) => set({ imageSrc: src }),
      scale: 1,
      setScale: (scale) => set({ scale }),
      texts: [],
      setTexts: (blocks) => set({ texts: blocks }),
      segment: null,
      setSegment: (segment) => set({ segment }),
      imageSrcHistory: [],
      setImageSrcHistory: (src) => {
        const history = get().imageSrcHistory
        if (history[history.length - 1] !== src) {
          if (history.length >= 5) {
            history.shift()
          }
          history.push(src)
        }
      },
    }),
    {
      name: 'canvas-storage',
      storage: createJSONStorage(() => createStorage('canvas-storage')),
    }
  )
)

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
      storage: createJSONStorage(() => createStorage('settings-storage')),
    }
  )
)
