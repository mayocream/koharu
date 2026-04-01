'use client'

export const QUERY_SCOPE = {
  documents: 'documents',
  llm: 'llm',
  providers: 'providers',
  system: 'system',
} as const

export const matchesScopedQueryKey = (
  queryKey: readonly unknown[],
  root: string,
  scope?: string,
) => queryKey[0] === root && (scope === undefined || queryKey[1] === scope)
