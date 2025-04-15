import Konva from 'konva'
import { create } from 'zustand'
import { combine } from 'zustand/middleware'

export const useStageStore = create(
  combine({ stage: null as Konva.Stage }, (set) => ({
    setStage: (stage: Konva.Stage) => set({ stage }),
  }))
)
