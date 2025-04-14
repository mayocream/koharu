import { useState } from 'react'
import { Move, Type } from 'lucide-react'

function Tools() {
  const [selectedTool, setSelectedTool] = useState('move')

  const tools = [
    {
      id: 'move',
      name: '移動ツール',
      icon: Move,
      description: 'サイズ変更や移動ができます',
    },
    {
      id: 'text',
      name: 'テキストツール',
      icon: Type,
      description: 'テキストを追加できます',
    },
  ]

  return (
    <div className='w-14 bg-gray-50 border border-gray-200 rounded-xl shadow-sm py-2 flex flex-col items-center'>
      {tools.map((tool) => (
        <div
          key={tool.id}
          className={`w-10 h-10 mb-4 flex items-center justify-center rounded-lg cursor-pointer ${
            selectedTool === tool.id
              ? 'bg-blue-500 text-white'
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
