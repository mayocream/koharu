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
  openDocuments: () => Promise<void>
  openExternal: (url: string) => Promise<void>
  setCurrentDocumentIndex?: (index: number) => void
  setScale: (scale: number) => void
  setShowSegmentationMask: (show: boolean) => void
  setShowInpaintedImage: (show: boolean) => void
  detect: (confThreshold: number, nmsThreshold: number) => Promise<void>
  ocr: () => Promise<void>
  inpaint: () => Promise<void>
}

export const useAppStore = create<AppState>((set, get) => ({
  documents: [],
  currentDocumentIndex: 0,
  scale: 100,
  showSegmentationMask: false,
  showInpaintedImage: false,
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
  inpaint: async () => {
    const index = get().currentDocumentIndex
    const doc: Document = await invoke('inpaint', { index })
    set({
      documents: [
        ...get().documents.slice(0, index),
        doc,
        ...get().documents.slice(index + 1),
      ],
    })
  },
}))
