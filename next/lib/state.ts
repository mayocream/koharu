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
    }),
    {
      name: 'canvas-storage',
      storage: createJSONStorage(() => createStorage('canvas-storage')),
    }
  )
)
