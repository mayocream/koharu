'use client'

import { useEffect, useState, type ReactNode } from 'react'

export function ClientOnly({
  children,
  fallback = null,
}: {
  children: ReactNode
  fallback?: ReactNode
}) {
  const [mounted, setMounted] = useState(false)

  useEffect(() => {
    setMounted(true)
  }, [])

  return mounted ? <>{children}</> : <>{fallback}</>
}

export default ClientOnly
