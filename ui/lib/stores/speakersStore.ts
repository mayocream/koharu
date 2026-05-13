'use client'

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

type SpeakersState = {
  speakersByProject: Record<string, string[]>
  addSpeaker: (projectId: string, name: string) => void
  removeSpeaker: (projectId: string, name: string) => void
  renameSpeaker: (projectId: string, oldName: string, newName: string) => void
  reorderSpeakers: (projectId: string, newOrder: string[]) => void
  getSpeakers: (projectId: string) => string[]
}

export const useSpeakersStore = create<SpeakersState>()(
  persist(
    (set, get) => ({
      speakersByProject: {},

      addSpeaker: (projectId, name) => {
        const trimmed = name.trim()
        if (!trimmed) return
        const current = get().speakersByProject[projectId] ?? []
        if (current.includes(trimmed)) return
        set((state) => ({
          speakersByProject: {
            ...state.speakersByProject,
            [projectId]: [...current, trimmed],
          },
        }))
      },

      removeSpeaker: (projectId, name) => {
        const current = get().speakersByProject[projectId] ?? []
        set((state) => ({
          speakersByProject: {
            ...state.speakersByProject,
            [projectId]: current.filter((n) => n !== name),
          },
        }))
      },

      renameSpeaker: (projectId, oldName, newName) => {
        const trimmed = newName.trim()
        if (!trimmed || trimmed === oldName) return
        const current = get().speakersByProject[projectId] ?? []
        if (current.includes(trimmed)) return
        set((state) => ({
          speakersByProject: {
            ...state.speakersByProject,
            [projectId]: current.map((n) => (n === oldName ? trimmed : n)),
          },
        }))
      },

      reorderSpeakers: (projectId, newOrder) => {
        set((state) => ({
          speakersByProject: {
            ...state.speakersByProject,
            [projectId]: newOrder,
          },
        }))
      },

      getSpeakers: (projectId) => {
        return get().speakersByProject[projectId] ?? []
      },
    }),
    {
      name: 'koharu-speakers',
    },
  ),
)