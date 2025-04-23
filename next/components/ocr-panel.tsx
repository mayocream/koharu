import { useCanvasStore, useWorkflowStore } from '@/lib/state'
import { Check, Loader, Pencil, Play, X } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { debug } from '@tauri-apps/plugin-log'

function OCRPanel() {
  const { imageSrc, texts, setTexts } = useCanvasStore()
  const { selectedTextIndex, setSelectedTextIndex } = useWorkflowStore()
  const [loading, setLoading] = useState(false)
  const [imageData, setImageData] = useState<ImageBitmap | null>(null)
  const [isTextEditMode, setIsTextEditMode] = useState(false)
  const [editedText, setEditedText] = useState([])

  const cropImage = async (
    xmin: number,
    ymin: number,
    xmax: number,
    ymax: number
  ) => {
    const width = xmax - xmin
    const height = ymax - ymin
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d')!

    ctx.drawImage(imageData, xmin, ymin, width, height, 0, 0, width, height)
    const croppedImage = await canvas.convertToBlob()
    return await croppedImage.arrayBuffer()
  }

  const inference = async () => {
    if (!imageData || texts.length === 0) return

    debug(`Starting OCR inference...`)

    setLoading(true)
    setIsTextEditMode(false)
    const newTexts = await Promise.all(
      texts.map(async (block) => {
        const { xmin, ymin, xmax, ymax } = block
        const croppedImageBuffer = await cropImage(xmin, ymin, xmax, ymax)
        const result = await invoke('ocr', {
          image: croppedImageBuffer,
        })
        return {
          ...block,
          text: result,
        }
      })
    )
    setTexts(newTexts)
    setLoading(false)

    debug(`OCR result: ${JSON.stringify(newTexts)}`)
  }

  const loadImage = async (src: string) => {
    if (!src) return

    const blob = await fetch(src).then((res) => res.blob())
    const bitmap = await createImageBitmap(blob)
    setImageData(bitmap)
  }

  const handleEditModeChange = () => {
    setEditedText(texts.map((block) => block.text))
    setIsTextEditMode(true)
  }

  const handleTextEdit = (index: number, value: string) => {
    const newTexts = [...editedText]
    newTexts[index] = value
    setEditedText(newTexts)
  }
  const handleTextSave = () => {
    const newTexts = texts.map((block, index) => ({
      ...block,
      text: editedText[index],
    }))
    setTexts(newTexts)
    setIsTextEditMode(false)
  }

  const handleTextUnsave = () => {
    setEditedText(texts.map((block) => block.text))
    setIsTextEditMode(false)
  }

  useEffect(() => {
    loadImage(imageSrc)
  }, [imageSrc])

  useEffect(() => {
    if (texts.length && texts.every((block) => !block.text)) {
      inference()
    }
  }, [texts])

  return (
    <div className='flex w-72 flex-col rounded-lg border border-gray-200 bg-white shadow-md'>
      {/* Header */}
      <div className='flex items-center p-3'>
        <h2 className='font-medium'>OCR</h2>
        <div className='flex-grow'></div>
        {texts.length > 0 &&
          !loading &&
          (isTextEditMode ? (
            <>
              <button
                className='cursor-pointer rounded-full p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
                onClick={handleTextUnsave}
                disabled={loading}
              >
                <X className='h-4 w-4' />
              </button>
              <button
                className='cursor-pointer rounded-full p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
                onClick={handleTextSave}
                disabled={loading}
              >
                <Check className='h-4 w-4' />
              </button>
            </>
          ) : (
            <button
              className='cursor-pointer rounded-full p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
              onClick={handleEditModeChange}
              disabled={loading}
            >
              <Pencil className='h-4 w-4' />
            </button>
          ))}
        {!isTextEditMode && (
          <button
            className='cursor-pointer rounded-full p-2 text-gray-500 hover:bg-gray-100 hover:text-gray-700'
            onClick={inference}
            disabled={loading}
          >
            {loading ? (
              <Loader className='h-4 w-4 animate-spin' />
            ) : (
              <Play className='h-4 w-4' />
            )}
          </button>
        )}
      </div>
      <div className='flex flex-col justify-center'>
        {texts.map((block, index) => (
          <div
            key={index}
            className='cursor-pointer border-b border-gray-200 px-4 py-2 text-sm'
            style={{
              backgroundColor:
                selectedTextIndex === index ? 'rgba(147, 140, 140, 0.3)' : '',
            }}
            onMouseEnter={() => setSelectedTextIndex(index)}
            onMouseLeave={() => setSelectedTextIndex(null)}
          >
            {isTextEditMode ? (
              <textarea
                value={editedText[index]}
                onChange={(e) => handleTextEdit(index, e.target.value)}
                className='w-full resize-none rounded border border-gray-200 bg-transparent leading-snug focus:border-gray-400 focus:outline-none'
              />
            ) : (
              block.text || '検出されていません'
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

export default OCRPanel
