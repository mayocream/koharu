import LayerPanel from './components/layer-panel'
import Tools from './components/tools'
import Topbar from './components/topbar'
import { StageProvider } from './components/stage-provider'
import Canvas from './components/canvas'

function App() {
  return (
    <StageProvider>
      <main className='flex flex-col h-screen w-screen bg-gray-100'>
        <Topbar />
        <div className='flex flex-1 my-2'>
          <Tools />

          <div className='flex-1 flex items-center justify-center'>
            <Canvas />
          </div>

          <div className='h-full'>
            <LayerPanel />
          </div>
        </div>
      </main>
    </StageProvider>
  )
}

export default App
