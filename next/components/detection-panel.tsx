import { useCanvasStore } from '@/lib/state'
import { Loader, Play } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { Button } from 'react-aria-components'

function DetectionPanel() {
  const { imageSrc, imageSrcHistory, texts, setTexts, setSegment } =
    useCanvasStore()
  const [loading, setLoading] = useState(false)
  const inference = async (src: string) => {
    setLoading(true)
    const buffer = await fetch(src).then((res) => res.bytes())
    const result = await invoke<any>('detect', {
      image: buffer,
    })

    if (imageSrcHistory[imageSrcHistory.length - 1] !== src) return
    setSegment(result.segment)

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
    if (imageSrc && texts.length === 0) {
      inference(imageSrc)
    }
  }, [imageSrc])

  return (
    <div className='flex w-72 flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>吹き出し検出</h2>
        <div className='flex-grow'></div>
        <Button
          className='cursor-pointer rounded-full p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
          onClick={() => inference(imageSrc)}
          isPending={loading}
        >
          {loading ? (
            <Loader className='h-4 w-4 animate-spin' />
          ) : (
            <Play className='h-4 w-4' />
          )}
        </Button>
      </div>
      {/* Body */}
      <div className='flex flex-col justify-center'>
        <div className='border-b border-gray-200 px-4 py-2 text-sm'>
          {texts.length} 個セリフを検出しました
        </div>
      </div>
    </div>
  )
}

export default DetectionPanel
