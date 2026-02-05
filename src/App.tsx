// src/App.tsx

import { useEffect, useState } from 'react'
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/core'
import { Theme } from '@radix-ui/themes'
import '@radix-ui/themes/styles.css'
import './App.css'

import { AppSidebar } from '@/components/AppSidebar'
import { SidebarProvider, SidebarInset } from '@/components/ui/sidebar'

import AuthPage from './pages/AuthPage'
import SignupPage from './pages/SignupPage'
import HomePage from './pages/HomePage'
import ChatPage from './pages/ChatPage'
import ProfilePage from './pages/ProfilePage'
import FriendsPage from './pages/FriendsPage'
import DirectMessagePage from './pages/DirectMessagePage'

// Check if running in Tauri
const isTauri = () => typeof window !== 'undefined' && '__TAURI__' in window

interface PublicSessionInfo {
  user_id: string
  email: string
  is_authenticated: boolean
}

function useSystemTheme() {
  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    if (typeof window !== 'undefined') {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    }
    return 'light'
  })

  useEffect(() => {
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = (e: MediaQueryListEvent) => setTheme(e.matches ? 'dark' : 'light')

    mediaQuery.addEventListener('change', handler)
    return () => mediaQuery.removeEventListener('change', handler)
  }, [])

  return theme
}

function AppLayout({
  children,
  showSidebar,
  onSignOut
}: {
  children: React.ReactNode
  showSidebar: boolean
  onSignOut: () => void
}) {
  if (!showSidebar) {
    return <>{children}</>
  }

  return (
    <SidebarProvider defaultOpen={false}>
      <AppSidebar onSignOut={onSignOut} />
      <SidebarInset>
        {children}
      </SidebarInset>
    </SidebarProvider>
  )
}

export default function App() {
  const systemTheme = useSystemTheme()
  const [loading, setLoading] = useState(true)
  const [session, setSession] = useState<PublicSessionInfo | null>(null)
  const [hasProfile, setHasProfile] = useState(false)

  useEffect(() => {
    const initialize = async () => {
      try {
        // Set up deep link listener for OAuth callback (Tauri only)
        if (isTauri()) {
          try {
            const { listen } = await import('@tauri-apps/api/event')
            await listen('deep-link', async (event) => {
              console.log('Deep link event received:', event.payload)
              const url = event.payload as string
              await handleOAuthCallback(url)
            })

            const { onOpenUrl } = await import('@tauri-apps/plugin-deep-link')
            await onOpenUrl(async (urls) => {
              console.log('Deep link received:', urls)
              for (const url of urls) {
                await handleOAuthCallback(url)
              }
            })
          } catch (err) {
            console.log('Deep link setup skipped:', err)
          }
        }

        // Get session from Rust backend (Cognito-backed)
        const currentSession = await invoke<PublicSessionInfo | null>('get_session')
        setSession(currentSession)

        if (currentSession?.user_id) {
          const profileExists = await invoke<boolean>('check_profile_exists')
          setHasProfile(profileExists)
        }
      } catch (error) {
        console.error('Failed to initialize:', error)
        setSession(null)
        setHasProfile(false)
      }

      setLoading(false)
    }

    // Handle OAuth callback URL — delegate token exchange to Rust backend
    const handleOAuthCallback = async (url: string) => {
      if (url.includes('auth/callback') || url.includes('code=')) {
        try {
          // Pass the full callback URL to the Rust backend.
          // The backend extracts the authorization code and exchanges it
          // with Cognito for tokens.
          await invoke('handle_oauth_callback', { callbackUrl: url })

          // Refresh session state after successful OAuth
          const currentSession = await invoke<PublicSessionInfo | null>('get_session')
          setSession(currentSession)

          if (currentSession?.user_id) {
            const profileExists = await invoke<boolean>('check_profile_exists')
            setHasProfile(profileExists)
          }
        } catch (err) {
          console.error('OAuth callback failed:', err)
        }
      }
    }

    initialize()
  }, [])

  // Function to handle sign out
  const handleSignOut = async () => {
    try {
      await invoke('sign_out')
      setSession(null)
      setHasProfile(false)
      window.location.href = '/'
    } catch (error) {
      console.error('Failed to sign out:', error)
    }
  }

  if (loading) {
    return <div>Loading...</div>
  }

  // Show sidebar only for authenticated users with a profile
  const showSidebar = !!session && hasProfile

  return (
    <Theme appearance={systemTheme}>
      <BrowserRouter>
        <AppLayout showSidebar={showSidebar} onSignOut={handleSignOut}>
          <Routes>
            <Route
              path="/"
              element={
                !session
                  ? <AuthPage />
                  : hasProfile
                    ? <Navigate to="/home" />
                    : <Navigate to="/profile" />
              }
            />

            <Route
              path="/signup"
              element={
                !session
                  ? <SignupPage />
                  : hasProfile
                    ? <Navigate to="/home" />
                    : <Navigate to="/profile" />
              }
            />

            {/* New user profile setup */}
            <Route
              path="/profile"
              element={
                !session
                  ? <Navigate to="/" />
                  : hasProfile
                    ? <Navigate to="/editProfile" />
                    : <ProfilePage />
              }
            />

            {/* Existing user profile editing */}
            <Route
              path="/editProfile"
              element={
                !session
                  ? <Navigate to="/" />
                  : !hasProfile
                    ? <Navigate to="/profile" />
                    : <ProfilePage />
              }
            />

            <Route
              path="/chat"
              element={
                !session
                  ? <Navigate to="/" />
                  : hasProfile
                    ? <ChatPage />
                    : <Navigate to="/profile" />
              }
            />

            {/* Home page - default landing */}
            <Route
              path="/home"
              element={
                !session
                  ? <Navigate to="/" />
                  : hasProfile
                    ? <HomePage />
                    : <Navigate to="/profile" />
              }
            />

            {/* Friends page */}
            <Route
              path="/friends"
              element={
                !session
                  ? <Navigate to="/" />
                  : hasProfile
                    ? <FriendsPage />
                    : <Navigate to="/profile" />
              }
            />

            {/* Direct message page */}
            <Route
              path="/chat/:friendId"
              element={
                !session
                  ? <Navigate to="/" />
                  : hasProfile
                    ? <DirectMessagePage />
                    : <Navigate to="/profile" />
              }
            />
          </Routes>
        </AppLayout>
      </BrowserRouter>
    </Theme>
  )
}