import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, vi } from 'vitest'

import { useEditorStore } from '@/lib/koharu'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? key,
    i18n: { language: 'en-US', changeLanguage: async () => undefined },
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
Element.prototype.releasePointerCapture = vi.fn()

const initial = useEditorStore.getState()
beforeEach(() => {
  useEditorStore.setState({
    ...initial,
    connection: 'connecting',
    revision: 0,
    project: null,
    pages: [],
    page: null,
    settings: null,
    jobs: {},
    error: null,
    notice: null,
    selectedElements: [],
    selectedPages: [],
    hoveredElement: null,
  })
})

afterEach(() => {
  delete window.koharu
  vi.restoreAllMocks()
})
