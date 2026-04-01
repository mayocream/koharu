'use client'

import { useQueryClient } from '@tanstack/react-query'
import { router } from 'react-query-kit'
import {
  initializeSystem as initializeRemoteSystem,
  updateConfig as updateRemoteConfig,
} from '@/lib/generated/orval/system/system'
import type { BootstrapConfig } from '@/lib/protocol'
import { QUERY_SCOPE } from '@/lib/react-query/scopes'
import { setBootstrapConfigCache } from '@/lib/system/queries'

export const systemMutations = router(QUERY_SCOPE.system, {
  config: {
    update: router.mutation<BootstrapConfig, BootstrapConfig>({
      mutationFn: async (config) => await updateRemoteConfig(config),
    }),
  },
  initialization: {
    start: router.mutation<void>({
      mutationFn: async () => {
        await initializeRemoteSystem()
      },
    }),
  },
})

export const useUpdateBootstrapConfigMutation = () => {
  const queryClient = useQueryClient()

  return systemMutations.config.update.useMutation({
    onSuccess: (saved) => {
      setBootstrapConfigCache(queryClient, saved)
    },
  })
}

export const useInitializeSystemMutation = () =>
  systemMutations.initialization.start.useMutation()
