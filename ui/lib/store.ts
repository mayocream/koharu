'use client'

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { Document } from '@/types'

// A mixin of application state, ui state and actions.
type AppState = {
  documents: Document[]
  currentDocumentIndex: number
  scale: number
  showSegmentationMask: boolean
  showInpaintedImage: boolean
  // LLM state
  llmModels: string[]
  llmSelectedModel?: string
  llmReady: boolean
  llmSystemPrompt: string
  // ui + actions
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  detect: (confThreshold: number, nmsThreshold: number) => Promise<void>
  ocr: () => Promise<void>
  inpaint: (dilateKernelSize: number, erodeDistance: number) => Promise<void>
  // LLM actions
  llmList: () => Promise<void>
  llmSetSelectedModel: (id: string) => void
  llmLoad: () => Promise<void>
  llmOffload: () => Promise<void>
  llmCheckReady: () => Promise<void>
  llmSetSystemPrompt: (prompt: string) => void
  llmGenerate: () => Promise<void>
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: false,
  llmModels: [],
  llmSelectedModel: undefined,
  llmReady: false,
  llmSystemPrompt:
    'You are a helpful assistant that rewrites extracted text cleanly.',
  openDocuments: async () => {
    const docs: Document[] = await invoke('open_documents')
    set({ documents: docs })
  },
  openExternal: async (url: string) => {
    await invoke('open_external', { url })
  },
  setCurrentDocumentIndex: (index: number) => {
    set({ currentDocumentIndex: index })
  },
  setScale: (scale: number) => {
    const clamped = Math.max(10, Math.min(200, Math.round(scale)))
    set({ scale: clamped })
  },
  setShowSegmentationMask: (show: boolean) => {
    set({ showSegmentationMask: show })
  },
  setShowInpaintedImage: (show: boolean) => {
    set({ showInpaintedImage: show })
  },
  detect: async (confThreshold: number, nmsThreshold: number) => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('detect', {
      index,
      confThreshold,
      nmsThreshold,
    })
    set({
      documents: [
        ...get().documents.slice(0, index),
        doc,
        ...get().documents.slice(index + 1),
      ],
    })
  },
  ocr: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('ocr', { index })
    set({
      documents: [
        ...get().documents.slice(0, index),
        doc,
        ...get().documents.slice(index + 1),
      ],
    })
  },
  inpaint: async (dilateKernelSize: number, erodeDistance: number) => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('inpaint', {
      index,
      dilateKernelSize,
      erodeDistance,
    })
    set({
      documents: [
        ...get().documents.slice(0, index),
        doc,
        ...get().documents.slice(index + 1),
      ],
    })
  },
  llmList: async () => {
    try {
      const models = await invoke<string[]>('llm_list')
      set({ llmModels: models })
      // Keep selected if still present, otherwise default to first
      const current = get().llmSelectedModel
      if (!current || !models.includes(current)) {
        set({ llmSelectedModel: models[0] })
      }
    } catch (_) {}
  },
  llmSetSelectedModel: (id: string) => set({ llmSelectedModel: id }),
  llmLoad: async () => {
    const id = get().llmSelectedModel
    if (!id) return
    await invoke('llm_load', { id })
  },
  llmOffload: async () => {
    await invoke('llm_offload')
    set({ llmReady: false })
  },
  llmCheckReady: async () => {
    try {
      const ready = await invoke<boolean>('llm_ready')
      set({ llmReady: ready })
    } catch (_) {}
  },
  llmSetSystemPrompt: (prompt: string) => set({ llmSystemPrompt: prompt }),
  llmGenerate: async () => {
    const { currentDocumentIndex, llmSystemPrompt } = get()
    const doc = await invoke<Document>('llm_generate', {
      index: currentDocumentIndex,
      prompt: llmSystemPrompt,
    })
    set({
      documents: [
        ...get().documents.slice(0, currentDocumentIndex),
        doc,
        ...get().documents.slice(currentDocumentIndex + 1),
      ],
    })
  },
}))
