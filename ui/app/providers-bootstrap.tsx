'use client'

import type { ReactNode } from 'react'
import { useAppRuntimeController } from '@/hooks/runtime/useAppRuntimeController'

export function ProvidersBootstrap({ children }: { children: ReactNode }) {
  useAppRuntimeController()

  return children
}
