import { Component, StrictMode, type ErrorInfo, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { PrivacyProvider } from './context/PrivacyContext.tsx'

interface Props {
  children: ReactNode
}

interface State {
  hasError: boolean
  error: Error | null
}

class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Unhandled UI error caught by ErrorBoundary:', error, errorInfo)
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          height: '100vh',
          width: '100vw',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          backgroundColor: '#07090e',
          color: '#f3f4f6',
          padding: '24px',
          boxSizing: 'border-box',
          fontFamily: 'system-ui, -apple-system, sans-serif'
        }}>
          <div style={{
            maxWidth: '540px',
            width: '100%',
            backgroundColor: '#0d1117',
            border: '1px solid rgba(239, 68, 68, 0.4)',
            borderRadius: '16px',
            padding: '24px',
            boxShadow: '0 20px 25px -5px rgba(0, 0, 0, 0.5)'
          }}>
            <h2 style={{ fontSize: '16px', fontWeight: 600, color: '#f87171', margin: '0 0 8px 0' }}>
              Application Render Error
            </h2>
            <p style={{ fontSize: '13px', color: '#9ca3af', margin: '0 0 16px 0', lineHeight: 1.5 }}>
              An unexpected error occurred while rendering the interface.
            </p>
            {this.state.error && (
              <pre style={{
                fontSize: '11px',
                backgroundColor: '#05070a',
                padding: '12px',
                borderRadius: '8px',
                color: '#fca5a5',
                overflowX: 'auto',
                marginBottom: '16px',
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-all'
              }}>
                {this.state.error.message || String(this.state.error)}
              </pre>
            )}
            <button
              onClick={() => window.location.reload()}
              style={{
                backgroundColor: '#2563eb',
                color: '#ffffff',
                border: 'none',
                padding: '8px 16px',
                borderRadius: '8px',
                fontSize: '13px',
                fontWeight: 500,
                cursor: 'pointer'
              }}
            >
              Reload Application
            </button>
          </div>
        </div>
      )
    }

    return this.props.children
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <PrivacyProvider>
        <App />
      </PrivacyProvider>
    </ErrorBoundary>
  </StrictMode>,
)
