'use client'

import { QueryCache, QueryClient } from '@tanstack/react-query'
import { persistQueryClient } from '@tanstack/react-query-persist-client'
import { createSyncStoragePersister } from '@tanstack/query-sync-storage-persister'
import { reportQueryError } from '@/lib/errors'
import { matchesScopedQueryKey, QUERY_SCOPE } from '@/lib/react-query/scopes'

const PERSIST_KEY = 'koharu-rq-v1'
const PERSIST_MAX_AGE = 24 * 60 * 60 * 1000

let client: QueryClient | null = null
let persistenceSetup = false

const shouldPersistQueryKey = (queryKey: readonly unknown[]) =>
  matchesScopedQueryKey(queryKey, QUERY_SCOPE.system, 'fonts') ||
  matchesScopedQueryKey(queryKey, QUERY_SCOPE.llm, 'models')

const createClient = () =>
  new QueryClient({
    queryCache: new QueryCache({
      onError: (error, query) => {
        reportQueryError(query, error)
      },
    }),
    defaultOptions: {
      queries: {
        gcTime: 5 * 60 * 1000,
        retry: 1,
        refetchOnReconnect: true,
        refetchOnWindowFocus: true,
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
