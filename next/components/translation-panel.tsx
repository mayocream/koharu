'use client'

import { Badge, Button, Text, TextArea } from '@radix-ui/themes'
import { Play } from 'lucide-react'
import { useState } from 'react'

function TranslationPanel() {
  const [loading, setLoading] = useState(false)

  const texts = []

  const translate = async () => {
    // TODO: Implement translation logic
  }

  return (
    <div className='flex max-h-[800px] w-full flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>Translation</h2>
        <div className='flex-grow'></div>
        <Button onClick={translate} loading={loading} variant='soft'>
          <Play className='h-4 w-4' />
        </Button>
      </div>
      {/* Body */}
      <div className='flex flex-col border-b border-gray-200 p-3'>
        <Text className='text-sm'>System Prompt</Text>
        <TextArea
          className='w-full rounded-md border border-gray-300 px-2 py-1 text-sm focus:ring-2 focus:ring-blue-500 focus:outline-none'
          placeholder='You are a manga translator, translate Japanese to English while preserve order of the text.'
        />
      </div>
      <div className='flex flex-col overflow-y-auto'>
        {texts.map((block, index) => (
          <div
            key={index}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm'
          >
            <Text className='flex gap-2'>
              <Badge>{index + 1}</Badge>
              {block.translatedText || 'No translation available'}
            </Text>
          </div>
        ))}
      </div>
    </div>
  )
}

export default TranslationPanel
