'use client'

import { useEffect, useState } from 'react'
import { subscribeRpcConnection } from '@/lib/backend'

export const useRpcConnection = () => {
  const [connected, setConnected] = useState(false)

  useEffect(() => subscribeRpcConnection(setConnected), [])

  return connected
}
