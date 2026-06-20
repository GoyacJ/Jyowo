import '@testing-library/jest-dom/vitest'

import { appI18n } from '@/shared/i18n/i18n'

void appI18n.changeLanguage('en-US')

window.scrollTo = () => {}
Element.prototype.scrollIntoView = () => {}

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

globalThis.ResizeObserver = ResizeObserverMock

Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
  value(contextId: string) {
    if (contextId !== '2d') {
      return null
    }

    return {
      font: '',
      measureText(text: string) {
        return {
          width: text.length * 8,
        }
      },
    } as unknown as CanvasRenderingContext2D
  },
})
