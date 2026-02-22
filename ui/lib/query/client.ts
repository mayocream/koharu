'use client'

import { QueryClient } from '@tanstack/react-query'
import { persistQueryClient } from '@tanstack/react-query-persist-client'
import { createSyncStoragePersister } from '@tanstack/query-sync-storage-persister'

const PERSIST_KEY = 'koharu-rq-v1'
const PERSIST_MAX_AGE = 24 * 60 * 60 * 1000

let client: QueryClient | null = null
let persistenceSetup = false

const shouldPersistQueryKey = (queryKey: readonly unknown[]) => {
  const root = queryKey[0]
  const second = queryKey[1]
  if (root === 'fonts') return true
  if (root === 'llm' && second === 'models') return true
  return false
}

const createClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: 5 * 60 * 1000,
        retry: 1,
        refetchOnWindowFocus: false,
      },
      mutations: {
        retry: false,
      },
    },
  })

const setupPersistence = (queryClient: QueryClient) => {
  if (persistenceSetup || typeof window === 'undefined') return
  persistenceSetup = true

  const persister = createSyncStoragePersister({
    key: PERSIST_KEY,
    storage: window.localStorage,
  })

  persistQueryClient({
    queryClient,
    persister,
    maxAge: PERSIST_MAX_AGE,
    dehydrateOptions: {
      shouldDehydrateQuery: (query) => shouldPersistQueryKey(query.queryKey),
    },
  })
}

export const getQueryClient = () => {
  if (!client) {
    client = createClient()
    setupPersistence(client)
  }
  return client
}
