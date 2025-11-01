'use client'
import { createContext, useContext, useState, ReactNode } from 'react'

type AppState = {
  files: Uint8Array[]
}

type AppContextType = {
  state: AppState
  setFiles: (files: Uint8Array[]) => void
}

const AppContext = createContext<AppContextType | undefined>(undefined)

export function AppProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppState>({
    files: [],
  })

  const setFiles = (files: Uint8Array[]) => {
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
