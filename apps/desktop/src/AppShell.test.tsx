import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { AppShell } from './AppShell'

afterEach(() => {
  cleanup()
})

describe('AppShell', () => {
  it('renders a stable loading shell before metadata has loaded', () => {
    render(<AppShell metadata={null} />)
    expect(screen.getByText(/Loading BI Forgeworks/i)).toBeTruthy()
  })

  it('renders the BI Forgeworks brand name once metadata is provided, with no native runtime dependency', () => {
    render(
      <AppShell
        metadata={{
          name: 'BI Forgeworks',
          identifier: 'com.biforgeworks.desktop',
          version: '0.0.1',
          tagline: 'Linux-native analytics engineering',
          stage: 'Developer Preview',
        }}
      />,
    )
    expect(screen.getByText('BI Forgeworks')).toBeTruthy()
  })
})
