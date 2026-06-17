import { type BundledLanguage, type BundledTheme, codeToHtml } from 'shiki'

export type HighlightCodeOptions = {
  lang?: BundledLanguage
  theme?: BundledTheme
}

const DEFAULT_LANG: BundledLanguage = 'markdown'
const DEFAULT_THEME: BundledTheme = 'github-light'
const htmlCache = new Map<string, Promise<string>>()

export function highlightCode(code: string, options: HighlightCodeOptions = {}) {
  const lang = options.lang ?? DEFAULT_LANG
  const theme = options.theme ?? DEFAULT_THEME
  const cacheKey = `${theme}:${lang}:${code}`
  const cached = htmlCache.get(cacheKey)

  if (cached) {
    return cached
  }

  const highlighted = codeToHtml(code, { lang, theme })
  htmlCache.set(cacheKey, highlighted)
  return highlighted
}
