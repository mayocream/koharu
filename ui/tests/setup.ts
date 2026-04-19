import '@testing-library/jest-dom/vitest'
import { createElement } from 'react'
import { afterAll, afterEach, beforeAll, beforeEach, vi } from 'vitest'

import { queryClient } from '@/lib/queryClient'

import { server } from './msw/server'

// Boot a Node-side MSW server for every test. Each test may use
// `server.use(...)` to layer extra handlers on top of the defaults.
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

// The shared React Query cache leaks across tests unless reset — component
// tests render into it so they can observe `lib/io/scene.ts` invalidations.
beforeEach(() => queryClient.clear())

// ---------------------------------------------------------------------------
// Global mocks: keep components focused on behaviour, not framework wiring.
// ---------------------------------------------------------------------------

// `useTranslation` → return the key verbatim so tests can match on stable,
// unique identifiers regardless of locale. Keys look like `welcome.new`.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en-US', changeLanguage: async () => {} },
  }),
  Trans: ({ i18nKey, children }: { i18nKey?: string; children?: unknown }) =>
    (i18nKey ?? (children as never)) as never,
  I18nextProvider: ({ children }: { children: unknown }) => children as never,
  // Consumed by `lib/i18n.ts` at import time.
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

// next/image — render as a plain <img>.
vi.mock('next/image', () => ({
  __esModule: true,
  default: (props: Record<string, unknown>) => {
    const { priority: _priority, ...rest } = props
    return createElement('img', rest)
  },
}))

// ResizeObserver/IntersectionObserver stubs for jsdom.
class StubObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return []
  }
}
Object.defineProperty(globalThis, 'ResizeObserver', {
  value: StubObserver,
  writable: true,
})
Object.defineProperty(globalThis, 'IntersectionObserver', {
  value: StubObserver,
  writable: true,
})

// Hotkeys + gesture libs call scroll/focus methods jsdom doesn't implement.
Element.prototype.scrollIntoView = vi.fn()
Element.prototype.releasePointerCapture = vi.fn()
Element.prototype.hasPointerCapture = vi.fn(() => false)

// jsdom doesn't implement `window.matchMedia`; next-themes + some radix bits
// expect it during SSR-safe code paths.
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})
