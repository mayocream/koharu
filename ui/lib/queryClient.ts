'use client'

import { QueryClient } from '@tanstack/react-query'

/**
 * Shared singleton QueryClient. React components mount it via the provider in
 * `app/providers.tsx`; non-React modules import it directly to run
 * invalidations after mutations (`queryClient.invalidateQueries(...)`).
 *
 * Keeping a single client across the app means imperative `applyCommand`-style
 * calls and React Query hooks share the exact same cache.
 */
export const queryClient = new QueryClient()
