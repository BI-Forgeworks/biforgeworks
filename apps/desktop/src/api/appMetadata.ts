import { invoke } from '@tauri-apps/api/core'

export interface AppMetadata {
  name: string
  identifier: string
  version: string
  tagline: string
  stage: string
}

export function getAppMetadata(): Promise<AppMetadata> {
  return invoke<AppMetadata>('get_app_metadata')
}
