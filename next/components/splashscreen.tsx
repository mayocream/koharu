'use client'

import { Progress } from '@radix-ui/themes'

export default function SplashScreen({ progress }: { progress: number }) {
  return (
    <div className='flex h-screen w-screen flex-col items-center justify-center bg-white'>
      <div className='text-center'>
        <h1 className='mb-2 text-4xl font-bold text-[#E0115F]'>Koharu</h1>
        <p className='text-sm text-[#E0115F]/80'>Initializing...</p>
        <Progress value={progress} className='mt-4 w-64' />
      </div>
    </div>
  )
}
