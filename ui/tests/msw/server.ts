import { setupServer } from 'msw/node'

import { getDefaultMock } from '@/lib/api/default/default.msw'

// Orval-generated handlers seed every endpoint with faker data. Individual
// tests override specific routes via `server.use(http.get(...))`.
export const server = setupServer(...getDefaultMock())
