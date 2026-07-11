import { ArtifactText } from './DiffPanel'

export function CommandPanel({
  loading,
  missing,
  text,
}: {
  loading: boolean
  missing: boolean
  text: string | null
}) {
  return (
    <ArtifactText
      empty="Select a command event to inspect its output."
      loading={loading}
      missing={missing}
      text={text}
    />
  )
}
