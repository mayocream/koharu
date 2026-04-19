'use client'

import { useGetSceneJson } from '@/lib/api/default/default'
import type { Scene } from '@/lib/api/schemas'

/**
 * Backend is the source of truth for the scene. Components read it through
 * this hook — which is just a thin wrapper around the orval-generated
 * `useGetSceneJson` query. Mutations must invalidate `getGetSceneJsonQueryKey`
 * for the UI to pick up changes (see `lib/io/scene.ts`).
 *
 * When no project is open, `GET /scene.json` returns 400; React Query stores
 * that as an error and `scene` is `null`.
 */
export function useScene(): { scene: Scene | null; epoch: number } {
  const { data, isError } = useGetSceneJson({
    query: {
      retry: false,
      staleTime: Infinity,
      gcTime: Infinity,
    },
  })
  // React Query preserves `data` across a failed refetch, so closing a
  // project would leave the stale scene visible until the cache is
  // manually cleared. Treat an error response (e.g. 400 "no project open")
  // as an explicit "no scene".
  if (isError) return { scene: null, epoch: 0 }
  return {
    scene: data?.scene ?? null,
    epoch: data?.epoch ?? 0,
  }
}
