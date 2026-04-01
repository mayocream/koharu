import { defineConfig } from 'orval'

export default defineConfig({
  koharu: {
    input: {
      target: './.generated/openapi.json',
    },
    output: {
      clean: true,
      client: 'react-query',
      mode: 'tags-split',
      target: './lib/generated/orval',
      override: {
        fetch: {
          includeHttpResponseReturnType: false,
        },
        mutator: {
          path: './lib/orval/custom-fetch.ts',
          name: 'customFetch',
        },
      },
    },
  },
})
