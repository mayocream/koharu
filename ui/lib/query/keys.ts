export const queryKeys = {
  documents: {
    all: ['documents'] as const,
    count: ['documents', 'count'] as const,
    currentRoot: ['documents', 'current'] as const,
    current: (index: number) => ['documents', 'current', index] as const,
    thumbnailRoot: ['documents', 'thumbnail'] as const,
    thumbnail: (documentsVersion: number, index: number) =>
      ['documents', 'thumbnail', documentsVersion, index] as const,
  },
  fonts: ['fonts'] as const,
  llm: {
    all: ['llm'] as const,
    models: (language: string) => ['llm', 'models', language] as const,
    ready: (selectedModel?: string) =>
      ['llm', 'ready', selectedModel ?? 'none'] as const,
  },
  device: {
    info: ['device', 'info'] as const,
  },
  app: {
    version: ['app', 'version'] as const,
  },
} as const
