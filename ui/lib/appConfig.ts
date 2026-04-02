import type {
  AppConfig,
  AppConfigUpdate,
  AppLlmProviderConfigUpdate,
} from '@/lib/api/schemas'

const providerUpdatesFromConfig = (
  config: AppConfig,
): AppLlmProviderConfigUpdate[] =>
  (config.llm?.providers ?? []).map((provider) => ({
    id: provider.id,
    baseUrl: provider.baseUrl?.trim() ? provider.baseUrl.trim() : null,
    apiKey: null,
    clearApiKey: false,
  }))

export const toAppConfigUpdate = (
  config: AppConfig,
  providerOverrides?: AppLlmProviderConfigUpdate[],
): AppConfigUpdate => {
  const providers = new Map(
    providerUpdatesFromConfig(config).map((provider) => [
      provider.id,
      provider,
    ]),
  )

  for (const override of providerOverrides ?? []) {
    const current = providers.get(override.id) ?? {
      id: override.id,
      baseUrl: null,
      apiKey: null,
      clearApiKey: false,
    }
    providers.set(override.id, {
      ...current,
      ...override,
      baseUrl:
        override.baseUrl !== undefined
          ? override.baseUrl?.trim() || null
          : current.baseUrl?.trim() || null,
    })
  }

  return {
    data: {
      path: config.data.path.trim(),
    },
    llm: {
      providers: [...providers.values()],
    },
  }
}
