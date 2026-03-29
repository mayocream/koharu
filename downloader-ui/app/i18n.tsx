'use client'

import React, { createContext, useContext, useState, useEffect, ReactNode } from 'react'

type Language = 'en' | 'zh'

const dictionaries = {
  en: {
    title: 'Downloader',
    subtitle: 'Manage core packages, local GGUF models, and recover network assets.',
    nav_deps: 'Dependencies',
    nav_llms: 'Local LLMs',
    nav_network: 'Network',
    
    deps_title: 'Core Dependencies',
    deps_desc: 'Runtime libraries & models required by the translation engine.',
    llms_title: 'Local LLM Models',
    llms_desc: 'Optional GGUF model weights. Download or remove as needed.',
    
    status_missing: 'Missing',
    status_ready: 'Ready',
    status_partial: 'Partial',
    status_failed: 'Failed',
    status_failed_validation: 'Failed Validation',
    status_busy: 'Busy',
    
    task_idle: 'Idle',
    task_running: 'Downloading',
    task_completed: 'Completed',
    task_failed: 'Failed',
    task_cancelled: 'Cancelled',
    
    action_download: 'Download',
    action_reinstall: 'Reinstall',
    action_retry: 'Retry',
    action_cancel: 'Cancel',
    action_delete: 'Delete',
    
    net_title: 'Network & Proxy',
    net_desc: 'Proxy settings for downloads only. Does not affect the main app.',
    net_proxy: 'Proxy URL',
    net_proxy_hint: 'e.g. http://127.0.0.1:8080 or socks5://127.0.0.1:8080',
    net_pypi: 'PyPI Mirror URL',
    net_pypi_hint: 'Custom PyPI index for Python package downloads.',
    net_github: 'GitHub Mirror URL',
    net_github_hint: 'Custom base URL for llama.cpp release downloads.',
    net_save: 'Save',
    net_check: 'Check',
    net_checking: 'Checking…',
    net_check_ok: 'Connected',
    
    maint_title: 'Directories',
    maint_desc: 'Open managed directories in the file explorer.',
    maint_open_rt: 'Runtime directory',
    maint_open_md: 'Model directory',
    
    delete_confirm: 'Are you sure you want to delete this item?',
    tauri_unavailable: 'Tauri bridge is not available.',
  },
  zh: {
    title: '下载管理',
    subtitle: '管理核心运行库、本地 GGUF 模型及网络配置。',
    nav_deps: '核心依赖',
    nav_llms: '本地模型',
    nav_network: '网络设置',
    
    deps_title: '核心依赖',
    deps_desc: '翻译引擎所需的运行时库和基础模型。',
    llms_title: '本地 LLM 模型',
    llms_desc: '可选的 GGUF 模型文件，按需下载或删除。',
    
    status_missing: '未安装',
    status_ready: '已就绪',
    status_partial: '不完整',
    status_failed: '校验失败',
    status_failed_validation: '校验失败',
    status_busy: '处理中',
    
    task_idle: '空闲',
    task_running: '下载中',
    task_completed: '已完成',
    task_failed: '失败',
    task_cancelled: '已取消',
    
    action_download: '下载',
    action_reinstall: '重新安装',
    action_retry: '重试',
    action_cancel: '取消',
    action_delete: '删除',
    
    net_title: '网络与代理',
    net_desc: '仅用于下载器的代理设置，不影响主程序。',
    net_proxy: '代理地址',
    net_proxy_hint: '如：http://127.0.0.1:8080 或 socks5://127.0.0.1:8080',
    net_pypi: 'PyPI 镜像地址',
    net_pypi_hint: '用于下载 Python 包的自定义镜像源。',
    net_github: 'GitHub 镜像地址',
    net_github_hint: '用于下载 llama.cpp 发布文件的自定义地址。',
    net_save: '保存',
    net_check: '检查',
    net_checking: '检查中…',
    net_check_ok: '已连通',
    
    maint_title: '目录',
    maint_desc: '在文件管理器中打开对应目录。',
    maint_open_rt: '运行时目录',
    maint_open_md: '模型目录',
    
    delete_confirm: '确定要删除此项吗？',
    tauri_unavailable: 'Tauri 桥接不可用。',
  }
}

type Dictionary = typeof dictionaries.en
type I18nContextType = {
  lang: Language
  setLang: (lang: Language) => void
  t: (key: keyof Dictionary) => string
}

const I18nContext = createContext<I18nContextType | null>(null)

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Language>('en')

  useEffect(() => {
    // 自动侦测初始语言
    const saved = localStorage.getItem('koharu-dl-lang') as Language
    if (saved === 'en' || saved === 'zh') {
      setLangState(saved)
    } else {
      const browserLang = navigator.language.toLowerCase()
      if (browserLang.startsWith('zh')) {
        setLangState('zh')
      }
    }
  }, [])

  const setLang = (newLang: Language) => {
    setLangState(newLang)
    localStorage.setItem('koharu-dl-lang', newLang)
  }

  const t = (key: keyof Dictionary) => {
    return dictionaries[lang][key] || dictionaries.en[key] || key
  }

  return (
    <I18nContext.Provider value={{ lang, setLang, t }}>
      {children}
    </I18nContext.Provider>
  )
}

export function useTranslation() {
  const context = useContext(I18nContext)
  if (!context) {
    throw new Error('useTranslation must be used within an I18nProvider')
  }
  return context
}



