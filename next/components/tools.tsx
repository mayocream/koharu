'use client'

import { Languages, MessageCircle, SquareDashedMousePointer } from 'lucide-react'
import { useWorkflowStore } from '@/lib/state'

function Tools() {
  const { selectedTool, setSelectedTool } = useWorkflowStore()

  const tools = [
    {
      id: 'detection',
      name: '検出ツール',
      icon: MessageCircle,
      description: '画像内のオブジェクトを検出します',
    },
    {
      id: 'translation',
      name: '翻訳ツール',
      icon: Languages,
      description: '画像内のテキストを翻訳します',
    },
    {
      id: 'segmentation',
      name: 'セグメンテーションツール',
      icon: SquareDashedMousePointer,
      description: '画像をセグメント化します',
    }
  ]

  return (
    <div className='flex w-14 flex-col items-center rounded-xl border border-gray-200 bg-gray-50 py-2 shadow-sm'>
      {tools.map((tool) => (
        <div
          key={tool.id}
          className={`mb-4 flex h-10 w-10 cursor-pointer items-center justify-center rounded-lg ${
            selectedTool === tool.id
              ? 'bg-blue-400 text-white'
              : 'text-gray-700 hover:bg-gray-200'
          }`}
          onClick={() => setSelectedTool(tool.id)}
          title={`${tool.id}: ${tool.description}`}
        >
          <tool.icon
            size={22}
            className={
              selectedTool === tool.id ? 'text-white' : 'text-gray-700'
            }
          />
        </div>
      ))}
    </div>
  )
}

export default Tools
