import { create } from 'zustand'
import { combine } from 'zustand/middleware'
import { Image } from './image'

export const useEditorStore = create(
  combine(
    {
      image: null,
      tool: 'detection',
      scale: 1,
    } as {
      image: Image | null
      tool: string
      scale: number
    },
    (set) => ({
      setImage: (image: Image | null) => set({ image }),
      setTool: (tool: string) => set({ tool }),
      setScale: (scale: number) => set({ scale }),
    })
  )
)
