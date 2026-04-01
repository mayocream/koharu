'use client'

import { reportRpcError } from '@/lib/errors'

export const withRpcError = async <T>(
  method: string,
  fn: () => Promise<T>,
): Promise<T> => {
  try {
    return await fn()
  } catch (error) {
    reportRpcError(method, error)
    throw error
  }
}
