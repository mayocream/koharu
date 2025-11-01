'use client'
import { createContext, useContext, useState, ReactNode } from 'react'

export type FileData = {
  filename: string
  buffer: Uint8Array
}

type AppState = {
  files: FileData[]
}

type AppContextType = {
  state: AppState
  setFiles: (files: FileData[]) => void
}

const AppContext = createContext<AppContextType | undefined>(undefined)

export function AppProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppState>({
    files: [],
  })

  const setFiles = (files: FileData[]) => {
    setState({ files })
  }

  return (
    <AppContext.Provider
      value={{
        state,
        setFiles,
      }}
    >
      {children}
    </AppContext.Provider>
  )
}

export function useApp() {
  const context = useContext(AppContext)
  if (context === undefined) {
    throw new Error('useApp must be used within an AppProvider')
  }
  return context
}
