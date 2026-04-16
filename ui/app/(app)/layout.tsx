'use client'

import { MenuBar } from '@/components/MenuBar'

export default function AppLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className='flex h-screen w-screen flex-col overflow-hidden bg-background'>
      <MenuBar />
      {children}
    </div>
  )
}
