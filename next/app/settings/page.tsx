'use client'

import { useSettingsStore } from '@/lib/state'
import { ArrowLeft } from 'lucide-react'
import { useRouter } from 'next/navigation'
import { Button } from 'react-aria-components'

export default function Settings() {
  const router = useRouter()

  const {
    openAIServer,
    setOpenAIServer,
    openAIToken,
    setOpenAIToken,
    openAIModel,
    setOpenAIModel,
  } = useSettingsStore()

  return (
    <div className='flex min-h-screen w-full flex-col bg-gray-100'>
      {/* Header with back button */}
      <div className='bg-white p-4 shadow-sm'>
        <div className='mx-auto flex max-w-7xl items-center'>
          <Button
            className='mr-4 cursor-pointer rounded-full p-2 transition-colors hover:bg-gray-100'
            onClick={() => router.replace('/')}
          >
            <ArrowLeft size={18} />
          </Button>
          <h1 className='text-xl'>設定</h1>
        </div>
      </div>

      {/* Main content */}
      <div className='flex-grow p-6'>
        <div className='mx-auto max-w-7xl'>
          <div className='rounded-lg bg-white p-6 shadow-md'>
            <h2 className='mb-6 text-xl font-semibold'>API 設定</h2>

            {/* Form inputs */}
            <div className='max-w-2xl space-y-6'>
              <div className='space-y-2'>
                <label
                  htmlFor='server-url'
                  className='block text-sm font-medium text-gray-700'
                >
                  OpenAI サーバーURL
                </label>
                <input
                  id='server-url'
                  type='text'
                  defaultValue={openAIServer}
                  onChange={(e) => setOpenAIServer(e.target.value)}
                  className='w-full rounded-md border border-gray-300 px-3 py-2 shadow-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
                  placeholder='https://api.openai.com'
                />
              </div>

              <div className='space-y-2'>
                <label
                  htmlFor='api-token'
                  className='block text-sm font-medium text-gray-700'
                >
                  APIトークン
                </label>
                <input
                  id='api-token'
                  type='password'
                  defaultValue={openAIToken}
                  onChange={(e) => setOpenAIToken(e.target.value)}
                  className='w-full rounded-md border border-gray-300 px-3 py-2 shadow-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
                  placeholder='sk-...'
                />
              </div>

              <div className='space-y-2'>
                <label
                  htmlFor='model'
                  className='block text-sm font-medium text-gray-700'
                >
                  モデル
                </label>
                <input
                  id='model'
                  defaultValue={openAIModel}
                  onChange={(e) => setOpenAIModel(e.target.value)}
                  className='w-full rounded-md border border-gray-300 px-3 py-2 shadow-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
                  placeholder='sakura-galtransl-7b-v3'
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
