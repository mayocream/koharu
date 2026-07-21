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

// `useBlobImage` caches ready-to-paint object URLs (`blob:` URLs) as query
// data. Object URLs pin their backing `Blob` (a full decoded image) in memory
// until explicitly revoked, so a cached URL that is merely garbage-collected
// still leaks the image. During a "process all pages" run every page mints new
// sprite/inpaint/render blobs, so without this the webview's memory climbs one
// full image per hash for the whole run. Revoke the URL when its query leaves
// the cache, tying the Blob's lifetime to the cache entry's.
queryClient.getQueryCache().subscribe((event) => {
  if (event.type !== 'removed') return
  const [kind] = event.query.queryKey as [unknown]
  if (kind !== 'blobImage') return
  const url = event.query.state.data
  if (typeof url === 'string' && url.startsWith('blob:')) {
    URL.revokeObjectURL(url)
  }
})
