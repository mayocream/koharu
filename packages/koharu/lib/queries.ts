'use client'

import {
  QueryClient,
  type QueryKey,
  queryOptions,
  useIsMutating,
  useMutation,
  useQuery,
} from '@tanstack/react-query'

import { call } from './backend'
import { commands, type PageImportSource } from './protocol'

export const projectKey = ['project'] as const
export const pagesKey = ['pages'] as const
export const pageKey = ['page'] as const
const importPagesKey = ['import-pages'] as const

const projectQuery = queryOptions({
  queryKey: projectKey,
  queryFn: () => call(commands.getProject),
})

const pagesQuery = queryOptions({
  queryKey: pagesKey,
  queryFn: () => call(commands.getPages),
})

const pageQuery = queryOptions({
  queryKey: pageKey,
  queryFn: () => call(commands.getPage),
})

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Number.POSITIVE_INFINITY,
      retry: false,
      refetchOnReconnect: false,
      refetchOnWindowFocus: false,
    },
  },
})

export function useProject(enabled = true) {
  return useQuery({ ...projectQuery, enabled })
}

export function usePages(enabled = true) {
  return useQuery({ ...pagesQuery, enabled })
}

export function usePage(enabled = true) {
  return useQuery({ ...pageQuery, enabled })
}

export function useImportPages() {
  const importing = useIsMutating({ mutationKey: importPagesKey }) > 0
  const mutation = useMutation({
    mutationKey: importPagesKey,
    mutationFn: (source: PageImportSource) => call(commands.importPages, source),
    onSuccess: () => refresh(projectKey, pagesKey, pageKey),
  })
  return { importPages: mutation.mutate, importing }
}

export async function refresh(...keys: QueryKey[]): Promise<void> {
  await Promise.all(keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })))
}
