'use client'

import { useSettingsStore } from '@/lib/state'
import { ArrowLeft, Save } from 'lucide-react'
import { useRouter } from 'next/navigation'

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
    <div className='flex flex-col min-h-screen w-full bg-gray-100'>
      {/* Header with back button */}
      <div className='bg-white shadow-sm p-4'>
        <div className='max-w-7xl mx-auto flex items-center'>
          <button
            className='mr-4 p-2 rounded-full hover:bg-gray-100 transition-colors cursor-pointer'
            onClick={() => router.replace('/')}
          >
            <ArrowLeft size={18} />
          </button>
          <h1 className='text-xl'>設定</h1>
        </div>
      </div>

      {/* Main content */}
      <div className='flex-grow p-6'>
        <div className='max-w-7xl mx-auto'>
          <div className='bg-white rounded-lg shadow-md p-6'>
            <h2 className='text-xl font-semibold mb-6'>API 設定</h2>

            {/* Form inputs */}
            <div className='space-y-6 max-w-2xl'>
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
                  className='w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500'
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
                  className='w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500'
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
                  className='w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500'
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
