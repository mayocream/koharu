import { useCanvasStore, useSettingsStore, useWorkflowStore } from '@/lib/state'
import { Badge, Button, Text, TextArea } from '@radix-ui/themes'
import { Play } from 'lucide-react'
import OpenAI from 'openai'
import { useState } from 'react'

function TranslationPanel() {
  const { texts, setTexts } = useCanvasStore()
  const { openAIServer, openAIToken, openAIModel } = useSettingsStore()
  const [loading, setLoading] = useState(false)
  const { prompt, setPrompt, selectedTextIndex, setSelectedTextIndex } =
    useWorkflowStore()

  const translate = async () => {
    setLoading(true)
    const client = new OpenAI({
      baseURL: openAIServer,
      apiKey: openAIToken,
      dangerouslyAllowBrowser: true,
    })

    const response = await client.chat.completions.create({
      model: openAIModel,
      messages: [
        {
          role: 'system',
          content: prompt,
        },
        {
          role: 'user',
          content: JSON.stringify(texts.map((block) => block.text)),
        },
      ],
      response_format: {
        type: 'json_schema',
        json_schema: {
          name: 'translation',
          description: 'Translated text',
          schema: {
            type: 'array',
            items: {
              type: 'string',
              description: 'Translated text for each block',
            },
          },
        },
      },
    })

    const translatedTexts = JSON.parse(response.choices[0].message.content)
    const newTexts = texts.map((block, index) => ({
      ...block,
      translatedText: translatedTexts[index] || '',
    }))
    setTexts(newTexts)

    setLoading(false)
  }

  return (
    <div className='flex h-[800px] w-full flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
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
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
        />
      </div>
      <div className='flex flex-col overflow-y-auto'>
        {texts.map((block, index) => (
          <div
            key={index}
            style={{
              backgroundColor:
                selectedTextIndex === index ? 'rgba(147, 140, 140, 0.3)' : '',
            }}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm'
            onMouseEnter={() => setSelectedTextIndex(index)}
            onMouseLeave={() => setSelectedTextIndex(null)}
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
