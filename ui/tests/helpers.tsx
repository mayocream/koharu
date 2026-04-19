import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, type RenderOptions } from '@testing-library/react'
import { type ReactElement, type ReactNode } from 'react'

import { queryClient as sharedQueryClient } from '@/lib/queryClient'

/**
 * Build a throwaway QueryClient for hook-only tests that want full isolation
 * (no shared cache leakage). For component tests, prefer
 * `renderWithQuery` which defaults to the app's module-level singleton so
 * `lib/io/scene.ts` invalidations actually flow into the UI under test.
 */
export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
}

export function withQueryClient(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

/**
 * Render a component under the shared app QueryClient (the same one
 * `lib/queryClient.ts` exports and `lib/io/scene.ts` invalidates). Pass
 * `client` to override for isolated scenarios. Callers must `.clear()` the
 * shared client in `beforeEach` to avoid cross-test bleed.
 */
export function renderWithQuery(
  ui: ReactElement,
  options?: Omit<RenderOptions, 'wrapper'> & { client?: QueryClient },
): { client: QueryClient } & ReturnType<typeof render> {
  const client = options?.client ?? sharedQueryClient
  return {
    client,
    ...render(ui, { wrapper: withQueryClient(client), ...options }),
  }
}
