import { defineConfig } from 'orval'

export default defineConfig({
  koharu: {
    input: './openapi.json',
    output: {
      target: './lib/api',
      schemas: './lib/api/schemas',
      client: 'react-query',
      mode: 'tags-split',
      baseUrl: '/api/v1',
      mock: {
        type: 'msw',
        delay: 0,
      },
      override: {
        fetch: {
          includeHttpResponseReturnType: false,
        },
        mutator: {
          path: './lib/api/fetch.ts',
          name: 'fetchApi',
        },
        operations: {
          createPages: {
            formData: true,
          },
          addImageLayer: {
            formData: true,
          },
        },
        query: {
          options: {
            gcTime: 5 * 60 * 1000,
            retry: 1,
          },
        },
      },
    },
  },
})
