'use client'

import { useQuery } from '@tanstack/react-query'
import { getRuntimeDownloadsOptions } from '@/lib/app/runtime/queries'

export const useRuntimeDownloads = () => {
  const query = useQuery(getRuntimeDownloadsOptions())
  return query.data ?? []
}
