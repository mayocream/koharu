'use client'

import { useEffect, useState } from 'react'
import { subscribeRpcConnectionStatus } from '@/lib/infra/runtime/event-client'

export const useRpcConnectionStatus = () => {
  const [connected, setConnected] = useState(false)

  useEffect(() => subscribeRpcConnectionStatus(setConnected), [])

  return connected
}
