import {
  getProviderApiKey as getProviderApiKeyRemote,
  setProviderApiKey as setProviderApiKeyRemote,
} from '@/lib/generated/orval/providers/providers'

export const getRemoteProviderApiKey = async (provider: string) =>
  (await getProviderApiKeyRemote(provider)).apiKey ?? null

export const getProviderApiKey = getRemoteProviderApiKey

export const setRemoteProviderApiKey = async (
  provider: string,
  apiKey: string,
) => await setProviderApiKeyRemote(provider, { apiKey })

export const setProviderApiKey = setRemoteProviderApiKey
