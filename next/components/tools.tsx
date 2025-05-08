'use client'

import {
  Languages,
  MessageCircle,
  PaintbrushVertical,
  SquareDashedMousePointer,
} from 'lucide-react'
import { useWorkflowStore } from '@/lib/state'
import { IconButton } from '@radix-ui/themes'

function Tools() {
  const { selectedTool, setSelectedTool } = useWorkflowStore()

  const tools = [
    {
      id: 'detection',
      icon: MessageCircle,
      description: 'Detect text blocks in the image',
    },
    {
      id: 'segmentation',
      icon: SquareDashedMousePointer,
      description: 'Segment text blocks',
    },
    {
      id: 'inpaint',
      icon: PaintbrushVertical,
      description: 'Inpaint text blocks',
    },
    {
      id: 'translation',
      icon: Languages,
      description: 'Translate text blocks',
    },
  ]

  return (
    <div className='flex w-14 flex-col items-center rounded-xl border border-gray-200 bg-gray-50 py-2 shadow-sm'>
      {tools.map((tool) => (
        <div className='my-2' key={tool.id}>
          <IconButton
            size='3'
            onClick={() => setSelectedTool(tool.id)}
            title={`${tool.id}: ${tool.description}`}
            variant={selectedTool === tool.id ? 'solid' : 'soft'}
          >
            <tool.icon size={22} />
          </IconButton>
        </div>
      ))}
    </div>
  )
}

export default Tools
