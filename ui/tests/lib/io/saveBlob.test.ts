import { describe, expect, it } from 'vitest'

import { filenameFromContentDisposition } from '@/lib/io/saveBlob'

describe('filenameFromContentDisposition', () => {
  it('returns undefined for null / empty', () => {
    expect(filenameFromContentDisposition(null)).toBeUndefined()
    expect(filenameFromContentDisposition('')).toBeUndefined()
  })

  it('parses RFC5987 filename*', () => {
    expect(filenameFromContentDisposition("attachment; filename*=UTF-8''my%20file.zip")).toBe(
      'my file.zip',
    )
  })

  it('parses quoted filename', () => {
    expect(filenameFromContentDisposition('attachment; filename="report.psd"')).toBe('report.psd')
  })

  it('parses unquoted filename', () => {
    expect(filenameFromContentDisposition('attachment; filename=report.psd')).toBe('report.psd')
  })

  it('prefers filename* when both are present', () => {
    const header = 'attachment; filename="ascii.zip"; filename*=UTF-8\'\'unicode.zip'
    expect(filenameFromContentDisposition(header)).toBe('unicode.zip')
  })

  it('returns undefined when no filename is found', () => {
    expect(filenameFromContentDisposition('attachment')).toBeUndefined()
  })
})
