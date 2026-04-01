'use client'

import { useMutation, useQueryClient } from '@tanstack/react-query'
import { setProviderApiKey } from '@/lib/infra/providers/api'
import { setProviderApiKeyCache } from '@/lib/app/llm/queries'

export const useProviderApiKeyMutation = () => {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async ({
      provider,
      apiKey,
    }: {
      provider: string
      apiKey: string
    }) => await setProviderApiKey(provider, apiKey),
    onSuccess: (_data, variables) => {
      setProviderApiKeyCache(queryClient, variables.provider, variables.apiKey)
    },
  })
}
