'use client'

import Link from 'next/link'
import { ChevronLeftIcon } from 'lucide-react'

type PageHeaderProps = {
  title: string
  backHref?: string
}

export function PageHeader({ title, backHref = '/' }: PageHeaderProps) {
  return (
    <div className='mb-8 flex items-center'>
      <Link
        href={backHref}
        prefetch={false}
        className='text-muted-foreground hover:bg-accent hover:text-foreground absolute -left-14 flex size-10 items-center justify-center rounded-full transition'
      >
        <ChevronLeftIcon className='size-6' />
      </Link>
      <h1 className='text-foreground text-2xl font-bold'>{title}</h1>
    </div>
  )
}
