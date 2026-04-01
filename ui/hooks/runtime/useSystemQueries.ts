'use client'

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  saveBootstrapConfig,
  startSystemInitialization,
} from '@/lib/app/system/commands'
import {
  getBootstrapConfigOptions,
  getSystemFontsOptions,
  getSystemMetaOptions,
  setBootstrapConfigCache,
} from '@/lib/app/system/queries'
import type { BootstrapConfig } from '@/lib/contracts/protocol'

export const useBootstrapConfigQuery = (enabled = true) =>
  useQuery(getBootstrapConfigOptions(enabled))

export const useSystemMetaQuery = (enabled = true) =>
  useQuery(getSystemMetaOptions(enabled))

export const useDeviceInfoQuery = (enabled = true) =>
  useQuery({
    ...getSystemMetaOptions(enabled),
    select: (meta) => ({ mlDevice: meta.mlDevice }),
  })

export const useAppVersionQuery = (enabled = true) =>
  useQuery({
    ...getSystemMetaOptions(enabled),
    select: (meta) => meta.version,
  })

export const useSystemFontsQuery = (enabled = true) =>
  useQuery(getSystemFontsOptions(enabled))

export const useUpdateBootstrapConfigMutation = () => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async (config: BootstrapConfig) =>
      await saveBootstrapConfig(config),
    onSuccess: (saved) => {
      setBootstrapConfigCache(queryClient, saved)
    },
  })
}

export const useInitializeSystemMutation = () =>
  useMutation({
    mutationFn: async () => await startSystemInitialization(),
  })
