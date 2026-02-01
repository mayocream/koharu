export type LlmModelInfo = {
  id: string
  languages: string[]
}

export const OPENAI_COMPATIBLE_MODEL_ID = 'openai-compatible'
export const OPENAI_DEFAULT_MODEL = 'gpt-4o-mini'

export const OPENAI_COMPATIBLE_MODEL: LlmModelInfo = {
  id: OPENAI_COMPATIBLE_MODEL_ID,
  languages: [],
}

export const isOpenAIModel = (modelId?: string) =>
  modelId === OPENAI_COMPATIBLE_MODEL_ID

export const isOpenAIConfigured = (endpoint: string, apiKey: string) =>
  endpoint.trim().length > 0 || apiKey.trim().length > 0

const normalizeOpenAIEndpoint = (endpoint: string) => {
  const baseEndpoint = endpoint.trim() || 'https://api.openai.com/v1/'

  const parsed = new URL(baseEndpoint)
  if (!parsed.pathname.endsWith('/chat/completions')) {
    const normalizedPath = parsed.pathname.replace(/\/$/, '')
    parsed.pathname = `${normalizedPath}/chat/completions`
  }
  return parsed.toString()
}

type OpenAIChoice = {
  message?: { content?: string }
  text?: string
}

const parseOpenAIResponse = (payload: { choices?: OpenAIChoice[] }) => {
  const choice = payload?.choices?.[0]
  const content = choice?.message?.content ?? choice?.text
  if (!content) {
    throw new Error('OpenAI compatible response missing content')
  }
  return content.trim()
}

export const callOpenAICompletion = async ({
  endpoint,
  apiKey,
  prompt,
  content,
  model,
}: {
  endpoint: string
  apiKey: string
  prompt: string
  content: string
  model?: string
}) => {
  const resolvedEndpoint = endpoint.trim() || 'https://api.openai.com/v1/'
  const url = normalizeOpenAIEndpoint(resolvedEndpoint)
  if (resolvedEndpoint === 'https://api.openai.com/v1/' && !apiKey.trim()) {
    throw new Error('OpenAI compatible API key is required')
  }
  const parsed = new URL(url)
  const selectedModel =
    model?.trim() || parsed.searchParams.get('model') || OPENAI_DEFAULT_MODEL
  parsed.searchParams.delete('model')

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  }
  if (apiKey && apiKey.trim()) {
    headers.Authorization = `Bearer ${apiKey}`
  }

  const response = await fetch(parsed.toString(), {
    method: 'POST',
    headers,
    body: JSON.stringify({
      model: selectedModel,
      messages: [
        { role: 'system', content: prompt },
        { role: 'user', content },
      ],
      temperature: 0.2,
    }),
  })

  if (!response.ok) {
    const message = await response.text()
    throw new Error(
      `OpenAI compatible request failed (${response.status}): ${message}`,
    )
  }

  const payload = (await response.json()) as { choices?: OpenAIChoice[] }
  return parseOpenAIResponse(payload)
}
