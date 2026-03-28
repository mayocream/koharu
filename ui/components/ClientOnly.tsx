'use client'

import { useEffect, useState, type ReactNode } from 'react'

export function ClientOnly({ children }: { children: ReactNode }) {
  const [mounted, setMounted] = useState(false)

  useEffect(() => {
    setMounted(true)
  }, [])

  return mounted ? <>{children}</> : null
}

export default ClientOnly
