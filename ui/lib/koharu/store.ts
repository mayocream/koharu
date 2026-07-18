'use client'

import { create } from 'zustand'
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware'

import type {
  CanvasDisplay,
  DownloadStatus,
  ElementId,
  JobStatus,
  PageId,
  PageSummary,
  PageView,
  ProjectHeader,
  SettingsView,
  UiEvent,
} from './protocol'

export type ConnectionState = 'connecting' | 'connected' | 'disconnected'
export type EditorTool = 'select' | 'text' | 'text_mask' | 'brush_mask' | 'pan'
export type ShortcutAction = EditorTool | 'fit'
export type EditorShortcuts = Record<ShortcutAction, string>

interface EditorStore {
  connection: ConnectionState
  revision: number
  project: ProjectHeader | null
  pages: PageSummary[]
  page: PageView | null
  settings: SettingsView | null
  jobs: Record<string, JobStatus>
  downloads: Record<string, DownloadStatus>
  camera: { zoom: number; translation: [number, number]; autoFit: boolean }
  error: string | null
  notice: string | null
  selectedElements: ElementId[]
  selectedPages: PageId[]
  hoveredElement: ElementId | null
  tool: EditorTool
  brushSize: number
  erase: boolean
  display: CanvasDisplay
  showTextBounds: boolean
  targetLanguage: string
  instructions: string
  settingsOpen: boolean
  showNavigator: boolean
  shortcuts: EditorShortcuts
  setConnection: (connection: ConnectionState) => void
  setError: (error: string | null) => void
  setNotice: (notice: string | null) => void
  selectElements: (elements: ElementId[]) => void
  selectPages: (pages: PageId[]) => void
  setHoveredElement: (element: ElementId | null) => void
  setTool: (tool: EditorTool) => void
  setBrushSize: (size: number) => void
  setErase: (erase: boolean) => void
  setDisplay: (display: CanvasDisplay) => void
  setShowTextBounds: (show: boolean) => void
  setTargetLanguage: (language: string) => void
  setInstructions: (instructions: string) => void
  setSettingsOpen: (open: boolean) => void
  setShowNavigator: (show: boolean) => void
  setShortcut: (action: ShortcutAction, key: string) => void
  dismissJob: (id: string) => void
  dismissDownload: (id: number) => void
}

const defaultDisplay: CanvasDisplay = {
  page: 'source',
  show_text: true,
  text_mask: null,
  brush_mask: null,
}

const defaultShortcuts: EditorShortcuts = {
  select: 'v',
  text: 't',
  text_mask: 'm',
  brush_mask: 'b',
  pan: 'h',
  fit: '0',
}

const memoryValues = new Map<string, string>()
const memoryStorage: StateStorage = {
  getItem: (name) => memoryValues.get(name) ?? null,
  setItem: (name, value) => {
    memoryValues.set(name, value)
  },
  removeItem: (name) => {
    memoryValues.delete(name)
  },
}

function preferenceStorage(): StateStorage {
  if (typeof window !== 'undefined' && typeof window.localStorage?.setItem === 'function') {
    return window.localStorage
  }
  return memoryStorage
}

export const useEditorStore = create<EditorStore>()(
  persist(
    (set) => ({
      connection: 'connecting',
      revision: 0,
      project: null,
      pages: [],
      page: null,
      settings: null,
      jobs: {},
      downloads: {},
      camera: { zoom: 1, translation: [0, 0], autoFit: true },
      error: null,
      notice: null,
      selectedElements: [],
      selectedPages: [],
      hoveredElement: null,
      tool: 'select',
      brushSize: 48,
      erase: false,
      display: defaultDisplay,
      showTextBounds: false,
      targetLanguage: 'en-US',
      instructions: '',
      settingsOpen: false,
      showNavigator: true,
      shortcuts: defaultShortcuts,
      setConnection: (connection) => set({ connection }),
      setError: (error) => set({ error }),
      setNotice: (notice) => set({ notice }),
      selectElements: (selectedElements) =>
        set({ selectedElements: [...new Set(selectedElements)] }),
      selectPages: (selectedPages) => set({ selectedPages: [...new Set(selectedPages)] }),
      setHoveredElement: (hoveredElement) => set({ hoveredElement }),
      setTool: (tool) => set({ tool }),
      setBrushSize: (brushSize) => set({ brushSize: Math.min(512, Math.max(1, brushSize)) }),
      setErase: (erase) => set({ erase }),
      setDisplay: (display) => set({ display }),
      setShowTextBounds: (showTextBounds) => set({ showTextBounds }),
      setTargetLanguage: (targetLanguage) => set({ targetLanguage }),
      setInstructions: (instructions) => set({ instructions }),
      setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
      setShowNavigator: (showNavigator) => set({ showNavigator }),
      setShortcut: (action, key) =>
        set((state) => ({
          shortcuts: { ...state.shortcuts, [action]: key.toLowerCase().slice(0, 1) },
        })),
      dismissJob: (id) =>
        set((state) => {
          const jobs = { ...state.jobs }
          delete jobs[id]
          return { jobs }
        }),
      dismissDownload: (id) =>
        set((state) => {
          const downloads = { ...state.downloads }
          delete downloads[id]
          return { downloads }
        }),
    }),
    {
      name: 'koharu-editor-preferences-v1',
      storage: createJSONStorage(preferenceStorage),
      partialize: (state) => ({
        tool: state.tool,
        brushSize: state.brushSize,
        erase: state.erase,
        display: state.display,
        showTextBounds: state.showTextBounds,
        targetLanguage: state.targetLanguage,
        instructions: state.instructions,
        showNavigator: state.showNavigator,
        shortcuts: state.shortcuts,
      }),
    },
  ),
)

export function dispatchEvent(event: UiEvent): boolean {
  let synchronize = false
  useEditorStore.setState((state) => {
    switch (event.type) {
      case 'accepted':
      case 'command_cancelled':
        return { revision: event.revision }
      case 'rejected':
        return {
          revision: event.error.current_revision ?? state.revision,
          error: event.error.message,
        }
      case 'problem':
        return { error: event.error.message }
      case 'project_closed':
        return {
          revision: 0,
          project: null,
          pages: [],
          page: null,
          selectedElements: [],
          selectedPages: [],
          hoveredElement: null,
          jobs: {},
        }
      case 'project_opened':
        return {
          revision: event.revision,
          project: event.project,
          pages: event.pages,
          page: null,
          selectedElements: [],
          selectedPages: event.project.visible_page ? [event.project.visible_page] : [],
          hoveredElement: null,
          jobs: {},
          error: null,
        }
      case 'page_loaded':
        if (!state.project || event.revision < state.revision) return {}
        return {
          revision: event.revision,
          page: event.page,
          project: { ...state.project, visible_page: event.page.id },
          selectedElements: [],
          selectedPages: state.selectedPages.includes(event.page.id)
            ? state.selectedPages
            : [event.page.id],
          hoveredElement: null,
        }
      case 'project_changed': {
        if (!state.project || state.revision !== event.from) {
          synchronize = true
          return {
            selectedElements: [],
            hoveredElement: null,
            error: 'The editor state changed unexpectedly. Reloading the current project state…',
          }
        }
        const summaries = new Map(state.pages.map((page) => [page.id, page]))
        for (const page of event.pages) summaries.set(page.id, page)
        for (const page of event.deleted_pages) summaries.delete(page)

        let currentPage = state.page
        if (event.visible_page && currentPage?.id === event.visible_page.id) {
          const elements = new Map(currentPage.elements.map((element) => [element.id, element]))
          for (const element of event.visible_page.elements) elements.set(element.id, element)
          for (const element of event.visible_page.deleted_elements) elements.delete(element)
          currentPage = {
            id: event.visible_page.id,
            name: event.visible_page.name,
            size: event.visible_page.size,
            source: event.visible_page.source,
            assets: event.visible_page.assets,
            elements: event.visible_page.element_order.flatMap((id) => {
              const element = elements.get(id)
              return element ? [element] : []
            }),
          }
        }
        if (currentPage && !event.page_order.includes(currentPage.id)) currentPage = null
        const existingElements = new Set(currentPage?.elements.map((element) => element.id) ?? [])
        return {
          revision: event.revision,
          project: {
            ...state.project,
            name: event.name,
            visible_page: currentPage?.id ?? null,
            can_undo: event.can_undo,
            can_redo: event.can_redo,
          },
          pages: event.page_order.flatMap((id) => {
            const page = summaries.get(id)
            return page ? [page] : []
          }),
          page: currentPage,
          selectedElements: state.selectedElements.filter((id) => existingElements.has(id)),
          selectedPages: state.selectedPages.filter((id) => event.page_order.includes(id)),
          hoveredElement:
            state.hoveredElement && existingElements.has(state.hoveredElement)
              ? state.hoveredElement
              : null,
        }
      }
      case 'view_changed':
        return {
          camera: {
            zoom: event.zoom,
            translation: event.translation,
            autoFit: event.auto_fit,
          },
        }
      case 'job_changed':
        return { jobs: { ...state.jobs, [event.id]: stripEventType(event) } }
      case 'download_changed': {
        const downloads = { ...state.downloads }
        if (event.state === 'finished') {
          delete downloads[event.id]
        } else {
          downloads[event.id] = stripDownloadEvent(event)
        }
        return { downloads }
      }
      case 'settings_changed':
        return { settings: event.settings }
      case 'garbage_collected':
        return {
          notice: `Removed ${event.blobs} unused blobs (${formatBytes(event.bytes)}).`,
        }
      case 'hit_test':
        return {}
    }
  })
  return synchronize
}

function stripEventType(event: Extract<UiEvent, { type: 'job_changed' }>): JobStatus {
  const { type: _type, ...status } = event
  return status
}

function stripDownloadEvent(event: Extract<UiEvent, { type: 'download_changed' }>): DownloadStatus {
  const { type: _type, ...status } = event
  return status
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`
}
