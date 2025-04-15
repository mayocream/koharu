import { createContext, RefObject, useContext, useRef, ReactNode } from 'react'
import Konva from 'konva'

const StageContext = createContext<RefObject<Konva.Stage> | null>(null)

export const StageProvider = ({ children }: { children: ReactNode }) => {
  const stageRef = useRef<Konva.Stage>(null)

  return (
    <StageContext.Provider value={stageRef}>{children}</StageContext.Provider>
  )
}

export const useStage = (): RefObject<Konva.Stage> => {
  const context = useContext(StageContext)
  if (context === null) {
    throw new Error('useStage must be used within a StageProvider')
  }
  return context
}
