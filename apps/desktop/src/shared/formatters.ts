export function formatTime(value: Date | string, locale?: string) {
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(toDate(value))
}

export function formatNumber(value: number, locale?: string) {
  return new Intl.NumberFormat(locale).format(value)
}

export function formatMilliseconds(
  value: number | undefined,
  unavailable: string,
  locale?: string,
) {
  return value === undefined ? unavailable : `${formatNumber(value, locale)} ms`
}

export function formatTokens(tokens: number, locale?: string) {
  return `${formatNumber(tokens, locale)} tokens`
}

function toDate(value: Date | string) {
  return value instanceof Date ? value : new Date(value)
}
