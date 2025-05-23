'use client'

import { useSettingsStore } from '@/lib/state'
import { Button, TextField } from '@radix-ui/themes'
import { ArrowLeft } from 'lucide-react'
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
    <div className='flex min-h-screen w-full flex-col bg-gray-100'>
      {/* Header with back button */}
      <div className='bg-white p-4 shadow-sm'>
        <div className='mx-auto flex max-w-7xl items-center gap-4'>
          <Button onClick={() => router.replace('/')} variant='ghost'>
            <ArrowLeft size={18} />
          </Button>
          <h1 className='text-xl'>Settings</h1>
        </div>
      </div>

      {/* Main content */}
      <div className='flex-grow p-6'>
        <div className='mx-auto max-w-7xl'>
          <div className='rounded-lg bg-white p-6 shadow-md'>
            <h2 className='mb-6 text-xl font-semibold'>API Settings</h2>

            <p className='mb-4 text-sm text-gray-500'>
              Configure OpenAI API settings for the translation model. You can
              use LM Studio or any other OpenAI-compatible API. If you are using
              LM Studio, please set the server URL to
              <code className='mx-1'>http://localhost:1234/v1</code>, and enable
              the CORS option in the LM Studio settings.
            </p>

            {/* Form inputs */}
            <div className='max-w-2xl space-y-6'>
              <div className='space-y-2'>
                <label
                  htmlFor='server-url'
                  className='block text-sm font-medium text-gray-700'
                >
                  OpenAI Server URL
                </label>
                <TextField.Root
                  size='3'
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
                  API Token
                </label>
                <TextField.Root
                  size='3'
                  id='api-token'
                  type='password'
                  defaultValue={openAIToken}
                  onChange={(e) => setOpenAIToken(e.target.value)}
                  className='w-full rounded-md border border-gray-300 px-3 py-2 shadow-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
                  placeholder='Leave empty for local server'
                />
              </div>

              <div className='space-y-2'>
                <label
                  htmlFor='model'
                  className='block text-sm font-medium text-gray-700'
                >
                  Model
                </label>
                <TextField.Root
                  size='3'
                  id='model'
                  defaultValue={openAIModel}
                  onChange={(e) => setOpenAIModel(e.target.value)}
                  className='w-full rounded-md border border-gray-300 px-3 py-2 shadow-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
                  placeholder='qwen3-32b@q4_k_m'
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
