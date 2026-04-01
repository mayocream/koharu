import {
  deleteLlmSession as deleteLlmSessionRemote,
  getLlmSession as getLlmSessionRemote,
  listLlmModels as listLlmModelsRemote,
  pingLlm as pingLlmRemote,
  setLlmSession as setLlmSessionRemote,
} from '@/lib/generated/orval/llm/llm'
import type {
  LlmLoadRequest,
  LlmModelInfo,
  LlmPingResponse,
  LlmState,
} from '@/lib/contracts/protocol'

export const listRemoteLlmModels = async (payload?: { language?: string }) =>
  (await listLlmModelsRemote(payload)) as LlmModelInfo[]

export const listLlmModels = async (language?: string) =>
  await listRemoteLlmModels({ language })

export const getRemoteLlmSession = async () =>
  (await getLlmSessionRemote()) as LlmState

export const getLlmSession = getRemoteLlmSession

export const setRemoteLlmSession = async (payload: LlmLoadRequest) =>
  await setLlmSessionRemote(payload)

export const setLlmSession = setRemoteLlmSession

export const deleteRemoteLlmSession = async () => await deleteLlmSessionRemote()

export const deleteLlmSession = deleteRemoteLlmSession

export const pingRemoteLlm = async (payload: {
  baseUrl: string
  apiKey?: string
}) => (await pingLlmRemote(payload)) as LlmPingResponse

export const pingLlm = pingRemoteLlm
