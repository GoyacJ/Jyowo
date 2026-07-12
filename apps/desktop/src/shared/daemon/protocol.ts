import Ajv2020, { type ErrorObject, type ValidateFunction } from 'ajv/dist/2020'
import addFormats from 'ajv-formats'
import type { ClientFrame, ServerFrame } from '@/generated/daemon-protocol'
import schema from '@/generated/daemon-protocol.schema.json'

const ajv = new Ajv2020({
  allErrors: true,
  strict: true,
})
addFormats(ajv)
ajv.addFormat('ulid', /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/i)
ajv.addFormat('date-time', {
  validate: isStrictRfc3339,
})
ajv.addFormat('uint16', {
  type: 'number',
  validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= 65_535,
})
ajv.addFormat('uint32', {
  type: 'number',
  validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= 4_294_967_295,
})
ajv.addFormat('uint8', {
  type: 'number',
  validate: (value: number) => Number.isInteger(value) && value >= 0 && value <= 255,
})
ajv.addFormat('uint64', {
  type: 'number',
  validate: (value: number) => Number.isSafeInteger(value) && value >= 0,
})

const validateServerFrame = compileDefinition('ServerFrame')
const validateClientFrame = compileDefinition('ClientFrame')

export function parseServerFrame(value: unknown): ServerFrame {
  if (!validateServerFrame(value)) {
    throw validationError('server', validateServerFrame.errors)
  }
  return value as ServerFrame
}

export function parseClientFrame(value: unknown): ClientFrame {
  if (!validateClientFrame(value)) {
    throw validationError('client', validateClientFrame.errors)
  }
  return value as ClientFrame
}

function compileDefinition(name: 'ClientFrame' | 'ServerFrame'): ValidateFunction {
  return ajv.compile({
    $schema: schema.$schema,
    $ref: `#/$defs/${name}`,
    $defs: schema.$defs,
  })
}

function validationError(direction: 'client' | 'server', errors: ErrorObject[] | null | undefined) {
  const details = errors
    ?.map((error) => `${error.instancePath || '/'} ${error.message ?? 'is invalid'}`)
    .join('; ')
  return new Error(`Invalid daemon ${direction} frame${details ? `: ${details}` : ''}`)
}

function isStrictRfc3339(value: string) {
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:Z|[+-](\d{2}):(\d{2}))$/.exec(
      value,
    )
  if (!match) {
    return false
  }

  const [
    ,
    yearText,
    monthText,
    dayText,
    hourText,
    minuteText,
    secondText,
    offsetHour,
    offsetMinute,
  ] = match
  const year = Number(yearText)
  const month = Number(monthText)
  const day = Number(dayText)
  const daysInMonth = [31, isLeapYear(year) ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]

  return (
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= (daysInMonth[month - 1] ?? 0) &&
    Number(hourText) <= 23 &&
    Number(minuteText) <= 59 &&
    Number(secondText) <= 59 &&
    (offsetHour === undefined || Number(offsetHour) <= 23) &&
    (offsetMinute === undefined || Number(offsetMinute) <= 59)
  )
}

function isLeapYear(year: number) {
  return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)
}
