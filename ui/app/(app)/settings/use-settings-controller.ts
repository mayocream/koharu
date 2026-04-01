'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTheme } from 'next-themes'
import { pingLlm } from '@/lib/generated/orval/llm/llm'
import {
  createDebouncedAsyncTask,
  type DebouncedAsyncTask,
} from '@/lib/debounced-async'
import { reportAppError } from '@/lib/errors'
import { supportedLanguages } from '@/lib/i18n'
import type { LocalLlmPreset } from '@/lib/llm/presets'
import { useSetProviderApiKeyMutation } from '@/lib/llm/queries'
import { isTauri } from '@/lib/native'
import type { BootstrapConfig } from '@/lib/protocol'
import {
  getActivePresetConfig,
  type LocalLlmPresetConfig,
  usePreferencesStore,
} from '@/lib/stores/preferencesStore'
import { useUpdateBootstrapConfigMutation } from '@/lib/system/mutations'
import {
  useBootstrapConfigQuery,
  useDeviceInfoQuery,
} from '@/lib/system/queries'
import { API_PROVIDERS } from './settings-constants'

type PingResult = {
  ok: boolean
  count: number
  latency: number
  error?: string
}

export type PingState = {
  loading: boolean
  result?: PingResult
}

export const useSettingsController = () => {
  const { theme, setTheme } = useTheme()
  const tauri = isTauri()
  const { data: deviceInfo } = useDeviceInfoQuery(tauri)
  const bootstrapConfigQuery = useBootstrapConfigQuery()
  const setProviderApiKeyMutation = useSetProviderApiKeyMutation()
  const updateBootstrapConfigMutation = useUpdateBootstrapConfigMutation()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const localLlm = usePreferencesStore((state) => state.localLlm)
  const setLocalLlm = usePreferencesStore((state) => state.setLocalLlm)
  const setActivePreset = usePreferencesStore((state) => state.setActivePreset)
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({})
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [pingState, setPingState] = useState<PingState>({ loading: false })
  const [bootstrapConfig, setBootstrapConfig] =
    useState<BootstrapConfig | null>(null)

  const activeConfig = getActivePresetConfig(localLlm)

  useEffect(() => {
    if (!bootstrapConfigQuery.data) {
      return
    }

    setBootstrapConfig((current) => current ?? bootstrapConfigQuery.data)
  }, [bootstrapConfigQuery.data])

  useEffect(() => {
    if (!bootstrapConfigQuery.error) {
      return
    }

    reportAppError(bootstrapConfigQuery.error, {
      context: 'load configuration',
      dedupeKey: 'settings:load-config',
    })
  }, [bootstrapConfigQuery.error])

  const persistApiKey = useCallback(
    async (provider: string, value: string) => {
      try {
        await setProviderApiKeyMutation.mutateAsync({
          provider,
          apiKey: value,
        })
      } catch (error) {
        reportAppError(error, {
          context: `save the ${provider} API key`,
          dedupeKey: `settings:save-api-key:${provider}`,
        })
      }
    },
    [setProviderApiKeyMutation],
  )

  const persistBootstrapConfig = useCallback(
    async (nextConfig: BootstrapConfig) => {
      try {
        const saved =
          await updateBootstrapConfigMutation.mutateAsync(nextConfig)
        setBootstrapConfig(saved)
      } catch (error) {
        reportAppError(error, {
          context: 'save configuration',
          dedupeKey: 'settings:save-config',
        })
      }
    },
    [updateBootstrapConfigMutation],
  )

  const apiKeySavers = useMemo(
    () =>
      Object.fromEntries(
        API_PROVIDERS.map(({ id }) => [
          id,
          createDebouncedAsyncTask(async (value: string) => {
            await persistApiKey(id, value)
          }, 300),
        ]),
      ) as Record<string, DebouncedAsyncTask<[string]>>,
    [persistApiKey],
  )

  const proxySaver = useMemo(
    () => createDebouncedAsyncTask(persistBootstrapConfig, 300),
    [persistBootstrapConfig],
  )

  useEffect(() => {
    return () => {
      Object.values(apiKeySavers).forEach((save) => save.cancel())
      proxySaver.cancel()
    }
  }, [apiKeySavers, proxySaver])

  const toggleVisibleKey = useCallback((key: string) => {
    setVisibleKeys((current) => ({
      ...current,
      [key]: !current[key],
    }))
  }, [])

  const handleApiKeyChange = useCallback(
    (provider: string, value: string) => {
      setApiKey(provider, value)
      apiKeySavers[provider]?.run(value)
    },
    [apiKeySavers, setApiKey],
  )

  const flushApiKeySave = useCallback(
    async (provider: string) => {
      await apiKeySavers[provider]?.flush()
    },
    [apiKeySavers],
  )

  const handleProxyChange = useCallback(
    (value: string) => {
      if (!bootstrapConfig) {
        return
      }

      const nextConfig: BootstrapConfig = {
        ...bootstrapConfig,
        http: {
          proxy: value.trim() ? value : null,
        },
      }

      setBootstrapConfig(nextConfig)
      proxySaver.run(nextConfig)
    },
    [bootstrapConfig, proxySaver],
  )

  const flushProxySave = useCallback(async () => {
    await proxySaver.flush()
  }, [proxySaver])

  const handleTestConnection = useCallback(async () => {
    setPingState({ loading: true })

    try {
      const result = await pingLlm({
        baseUrl: activeConfig.baseUrl,
        apiKey: activeConfig.apiKey || undefined,
      })
      setPingState({
        loading: false,
        result: {
          ok: result.ok,
          count: result.models.length,
          latency: result.latencyMs ?? 0,
          error: result.error ?? undefined,
        },
      })
    } catch (error) {
      setPingState({
        loading: false,
        result: {
          ok: false,
          count: 0,
          latency: 0,
          error: String(error),
        },
      })
    }
  }, [activeConfig.apiKey, activeConfig.baseUrl])

  const handlePresetChange = useCallback(
    (preset: LocalLlmPreset) => {
      setActivePreset(preset)
      setPingState({ loading: false })
    },
    [setActivePreset],
  )

  const updateLocalLlm = useCallback(
    (config: Partial<LocalLlmPresetConfig>) => {
      setLocalLlm(config)
    },
    [setLocalLlm],
  )

  return {
    theme,
    setTheme,
    locales: supportedLanguages,
    deviceInfo,
    apiKeys,
    localLlm,
    activeConfig,
    visibleKeys,
    showAdvanced,
    setShowAdvanced,
    pingState,
    bootstrapConfig,
    toggleVisibleKey,
    handleApiKeyChange,
    flushApiKeySave,
    handleProxyChange,
    flushProxySave,
    handleTestConnection,
    handlePresetChange,
    updateLocalLlm,
  }
}
