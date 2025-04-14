import { Save, Image } from 'lucide-react'

function Topbar() {
  return (
    <div className='flex items-center p-2 bg-white border-b border-gray-200 shadow-sm'>
      <div className='flex items-center'>
        <button className='flex items-center p-2 mx-1 text-gray-700 hover:bg-gray-100 rounded'>
          <Image size={18} />
          <span className='ml-1'>ファイルを開く</span>
        </button>

        <button className='flex items-center p-2 mx-1 text-gray-700 hover:bg-gray-100 rounded'>
          <Save size={18} />
          <span className='ml-1'>保存</span>
        </button>
      </div>
    </div>
  )
}

export default Topbar
