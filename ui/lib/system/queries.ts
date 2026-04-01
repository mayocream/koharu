'use client'

import type { QueryClient } from '@tanstack/react-query'
import { router } from 'react-query-kit'
import {
  getConfig,
  getMeta,
  listFonts,
} from '@/lib/generated/orval/system/system'
import type { BootstrapConfig, FontFaceInfo, MetaInfo } from '@/lib/protocol'
import { matchesScopedQueryKey, QUERY_SCOPE } from '@/lib/react-query/scopes'

const SYSTEM_CONFIG_STALE_TIME = 10 * 60 * 1000
const SYSTEM_META_STALE_TIME = 10 * 60 * 1000
const SYSTEM_FONTS_STALE_TIME = 10 * 60 * 1000

export const systemQueries = router(QUERY_SCOPE.system, {
  config: router.query<BootstrapConfig>({
    fetcher: async () => await getConfig(),
    meta: {
      suppressGlobalError: true,
    },
  }),
  meta: router.query<MetaInfo>({
    fetcher: async () => await getMeta(),
    meta: {
      suppressGlobalError: true,
    },
  }),
  fonts: router.query<FontFaceInfo[]>({
    fetcher: async () => (await listFonts()) as FontFaceInfo[],
  }),
})

export const getBootstrapConfigQueryKey = () => systemQueries.config.getKey()

export const getCachedBootstrapConfig = (queryClient: QueryClient) =>
  queryClient.getQueryData<BootstrapConfig>(getBootstrapConfigQueryKey())

export const setBootstrapConfigCache = (
  queryClient: QueryClient,
  config: BootstrapConfig,
) => {
  queryClient.setQueryData(getBootstrapConfigQueryKey(), config)
}

export const shouldPersistSystemQueryKey = (queryKey: readonly unknown[]) =>
  matchesScopedQueryKey(queryKey, QUERY_SCOPE.system, 'fonts')

export const useBootstrapConfigQuery = (enabled = true) =>
  systemQueries.config.useQuery({
    enabled,
    staleTime: SYSTEM_CONFIG_STALE_TIME,
  })

export const useSystemMetaQuery = (enabled = true) =>
  systemQueries.meta.useQuery({
    enabled,
    staleTime: SYSTEM_META_STALE_TIME,
  })

export const useDeviceInfoQuery = (enabled = true) =>
  systemQueries.meta.useQuery<{ mlDevice: string }>({
    enabled,
    staleTime: SYSTEM_META_STALE_TIME,
    select: (meta) => ({ mlDevice: meta.mlDevice }),
  })

export const useAppVersionQuery = (enabled = true) =>
  systemQueries.meta.useQuery<string>({
    enabled,
    staleTime: SYSTEM_META_STALE_TIME,
    select: (meta) => meta.version,
  })

export const useSystemFontsQuery = (enabled = true) =>
  systemQueries.fonts.useQuery({
    enabled,
    staleTime: SYSTEM_FONTS_STALE_TIME,
  })
