export function getCommandErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  if (hasStringMessage(error)) {
    return error.message
  }

  return 'Unknown command error'
}

function hasStringMessage(error: unknown): error is { message: string } {
  return (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  )
}
