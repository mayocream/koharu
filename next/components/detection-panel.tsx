import { useCanvasStore } from '@/lib/state'
import { Loader, Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'

function DetectionPanel() {
  const { imageSrc, texts, setTexts } = useCanvasStore()
  const [loading, setLoading] = useState(false)
  const inference = async () => {
    setLoading(true)
    const buffer = await fetch(imageSrc).then((res) => res.bytes())
    const result = await invoke<any>('detect', {
      image: buffer,
    })

    result.bboxes.sort((a: any, b: any) => {
      const aCenter = (a.ymin + a.ymax) / 2
      const bCenter = (b.ymin + b.ymax) / 2
      return aCenter - bCenter
    })

    setTexts(result.bboxes)
    setLoading(false)
  }

  // auto trigger inference when imageSrc changes
  useEffect(() => {
    if (imageSrc) {
      inference()
    }
  }, [imageSrc])

  return (
    <div className='flex flex-col bg-white rounded-lg shadow-md w-72 border border-gray-200'>
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
      {/* Body */}
      <div className='flex flex-col justify-center'>
        <div className='border-b border-gray-200 py-2 px-4 text-sm'>
          {texts.length} 個セリフを検出しました
        </div>
      </div>
    </div>
  )
}

export default DetectionPanel
