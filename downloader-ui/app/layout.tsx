import './globals.css'
import type { Metadata } from 'next'
import { I18nProvider } from './i18n'

export const metadata: Metadata = {
  title: 'Koharu Downloader',
  description: 'Koharu dependency and model maintenance tool',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en'>
      <body>
        <I18nProvider>{children}</I18nProvider>
      </body>
    </html>
  )
}


