import { describe, expect, it } from 'vitest'

import { queryClient } from '@/lib/queryClient'

describe('queryClient', () => {
  it('is a QueryClient singleton', async () => {
    const mod = await import('@/lib/queryClient')
    expect(mod.queryClient).toBe(queryClient)
  })

  it('getQueryCache/mutationCache are available', () => {
    expect(typeof queryClient.getQueryCache().getAll).toBe('function')
    expect(typeof queryClient.getMutationCache().getAll).toBe('function')
  })
})
