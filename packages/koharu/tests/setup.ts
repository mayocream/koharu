import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, vi } from 'vitest'

import { queryClient } from '@/lib/queries'
import { defaultShortcuts, useKoharuStore } from '@/lib/store'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? key,
    i18n: {
      language: 'en-US',
      resolvedLanguage: 'en-US',
      changeLanguage: async () => undefined,
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => undefined },
  I18nextProvider: ({ children }: { children: React.ReactNode }) => children,
}))

class Observer {
  observe() {}
  unobserve() {}
  disconnect() {}
}

Object.defineProperty(globalThis, 'ResizeObserver', { value: Observer, writable: true })
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn((query: string) => ({
    matches: false,
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
  })),
})
Element.prototype.scrollIntoView = vi.fn()
Element.prototype.setPointerCapture = vi.fn()
Element.prototype.hasPointerCapture = vi.fn(() => true)
Element.prototype.releasePointerCapture = vi.fn()
Element.prototype.getAnimations = vi.fn(() => [])
Object.defineProperty(URL, 'createObjectURL', {
  value: vi.fn(() => 'blob:koharu-thumbnail'),
  writable: true,
})
Object.defineProperty(URL, 'revokeObjectURL', { value: vi.fn(), writable: true })

const initial = useKoharuStore.getState()
beforeEach(() => {
  queryClient.clear()
  useKoharuStore.setState({
    ...initial,
    initialized: false,
    preferences: null,
    translationModels: [],
    resources: null,
    jobs: {},
    downloads: {},
    camera: { zoom: 1, translation: [0, 0], fitted: true },
    layerFrames: {},
    selectedLayers: [],
    selectedPages: [],
    tool: 'select',
    brush: { diameter: 48, color: '#111111' },
    inspector: 'copy',
    settingsOpen: false,
    shortcuts: defaultShortcuts,
  })
})

afterEach(() => {
  vi.restoreAllMocks()
})
