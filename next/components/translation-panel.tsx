import { useCanvasStore, useSettingsStore, useWorkflowStore } from '@/lib/state'
import { Loader, Play } from 'lucide-react'
import OpenAI from 'openai'
import { useState } from 'react'

function TranslationPanel() {
  const { texts, setTexts } = useCanvasStore()
  const { openAIServer, openAIToken, openAIModel } = useSettingsStore()
  const [loading, setLoading] = useState(false)
  const { prompt, setPrompt } = useWorkflowStore()

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
          content: texts.map((block) => block.text).join('\n'),
        },
      ],
    })

    const translatedTexts = response.choices[0].message.content.split('\n')
    const newTexts = texts.map((block, index) => ({
      ...block,
      translatedText: translatedTexts[index],
    }))

    setTexts(newTexts)
    setLoading(false)
  }

  return (
    <div className='flex flex-col bg-white rounded-lg shadow-md w-72 max-h-160 overflow-auto border border-gray-200'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>翻訳</h2>
        <div className='flex-grow'></div>
        <button
          className='text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-full p-2 cursor-pointer'
          disabled={loading}
          onClick={translate}
        >
          {loading ? (
            <Loader className='w-4 h-4' />
          ) : (
            <Play className='w-4 h-4' />
          )}
        </button>
      </div>
      {/* Body */}
      <div className='flex items-center p-3 border-b border-gray-200'>
        <input
          type='text'
          className='w-full px-2 py-1 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 text-sm'
          placeholder='プロンプを入力'
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
        />
      </div>
      <div className='flex flex-col justify-center'>
        {texts.map((block, index) => (
          <div
            key={index}
            className='border-b border-gray-200 py-2 px-4 text-sm'
          >
            {block.translatedText || 'まだ翻訳されていません'}
          </div>
        ))}
      </div>
    </div>
  )
}

export default TranslationPanel
