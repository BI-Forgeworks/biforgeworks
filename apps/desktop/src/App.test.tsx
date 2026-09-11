import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { App } from './App'

vi.mock('./api/appMetadata', () => ({
  getAppMetadata: vi.fn().mockResolvedValue({
    name: 'BI Forgeworks',
    identifier: 'com.biforgeworks.desktop',
    version: '0.0.1',
    tagline: 'Linux-native analytics engineering',
    stage: 'Developer Preview',
  }),
}))

afterEach(() => {
  cleanup()
})

describe('App', () => {
  it('loads application metadata through the command boundary and renders the shell', async () => {
    render(<App />)

    expect(await screen.findByText('BI Forgeworks')).toBeTruthy()
    expect(screen.getByText('Linux-native analytics engineering')).toBeTruthy()
    expect(screen.getByText('Developer Preview')).toBeTruthy()
  })
})
