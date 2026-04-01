import type { QueryClient } from '@tanstack/react-query'
import type {
  BootstrapConfig,
  FontFaceInfo,
  MetaInfo,
} from '@/lib/contracts/protocol'
import { getConfig, getMeta, listFonts } from '@/lib/infra/system/api'
import { QUERY_ROOT } from '@/lib/app/query-keys'

const SYSTEM_CONFIG_STALE_TIME = 10 * 60 * 1000
const SYSTEM_META_STALE_TIME = 10 * 60 * 1000
const SYSTEM_FONTS_STALE_TIME = 10 * 60 * 1000

export const systemQueryKeys = {
  config: () => [QUERY_ROOT.system, 'config'] as const,
  meta: () => [QUERY_ROOT.system, 'meta'] as const,
  fonts: () => [QUERY_ROOT.system, 'fonts'] as const,
}

export const getBootstrapConfigOptions = (enabled = true) => ({
  queryKey: systemQueryKeys.config(),
  queryFn: async () => (await getConfig()) as BootstrapConfig,
  enabled,
  staleTime: SYSTEM_CONFIG_STALE_TIME,
  meta: {
    suppressGlobalError: true,
  },
})

export const getSystemMetaOptions = (enabled = true) => ({
  queryKey: systemQueryKeys.meta(),
  queryFn: async () => (await getMeta()) as MetaInfo,
  enabled,
  staleTime: SYSTEM_META_STALE_TIME,
  meta: {
    suppressGlobalError: true,
  },
})

export const getSystemFontsOptions = (enabled = true) => ({
  queryKey: systemQueryKeys.fonts(),
  queryFn: async () => (await listFonts()) as FontFaceInfo[],
  enabled,
  staleTime: SYSTEM_FONTS_STALE_TIME,
})

export const getBootstrapConfigQueryKey = () => systemQueryKeys.config()

export const getCachedBootstrapConfig = (queryClient: QueryClient) =>
  queryClient.getQueryData<BootstrapConfig>(getBootstrapConfigQueryKey())

export const setBootstrapConfigCache = (
  queryClient: QueryClient,
  config: BootstrapConfig,
) => {
  queryClient.setQueryData(getBootstrapConfigQueryKey(), config)
}
