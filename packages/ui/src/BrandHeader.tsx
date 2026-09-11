export interface BrandHeaderProps {
  name: string
  tagline: string
  stage: string
}

/**
 * Presentation-only branding header. Holds no product identity of its own —
 * name/tagline/stage are supplied by the caller, which sources them from the
 * Rust core crate via a Tauri command.
 */
export function BrandHeader({ name, tagline, stage }: BrandHeaderProps) {
  return (
    <>
      <h1>{name}</h1>
      <p>{tagline}</p>
      <span>{stage}</span>
    </>
  )
}
