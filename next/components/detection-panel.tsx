import { useCanvasStore } from '@/lib/state'
import { Loader, Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useState } from 'react'

function DetectionPanel() {
  const { imageSrc, setTexts: setBlocks } = useCanvasStore()
  const [loading, setLoading] = useState(false)
  const inference = async () => {
    setLoading(true)
    const buffer = await fetch(imageSrc).then((res) => res.bytes())
    const result = await invoke<any>('detect', {
      image: buffer,
    })

    setBlocks(result.bboxes)
    setLoading(false)
  }

  return (
    <div className='bg-white rounded-lg shadow-md w-72 border border-gray-200'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>吹き出し検出</h2>
        <div className='flex-grow'></div>
        <button
          className='text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-full p-2 cursor-pointer'
          onClick={inference}
          disabled={loading}
        >
          {loading ? (
            <Loader className='w-4 h-4' />
          ) : (
            <Play className='w-4 h-4' />
          )}
        </button>
      </div>
    </div>
  )
}

export default DetectionPanel
