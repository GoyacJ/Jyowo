import '@testing-library/jest-dom/vitest'

window.scrollTo = () => {}

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
