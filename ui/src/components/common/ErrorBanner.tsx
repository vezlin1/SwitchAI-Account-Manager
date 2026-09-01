import { AlertCircle, X } from 'lucide-react'

type ErrorBannerProps = {
  message: string
  onDismiss: () => void
}

export function ErrorBanner({ message, onDismiss }: ErrorBannerProps) {
  return (
    <div
      className="error-banner"
      role="alert"
      aria-live="polite"
      aria-atomic="true"
    >
      <div className="flex items-center gap-2.5 min-w-0 flex-1">
        <AlertCircle size={15} className="shrink-0 text-red-400" aria-hidden="true" />
        <span className="error-banner-message">{message}</span>
      </div>
      <button
        type="button"
        className="error-banner-dismiss"
        onClick={onDismiss}
        aria-label="Dismiss error"
        title="Dismiss error"
      >
        <X size={15} aria-hidden="true" />
      </button>
    </div>
  )
}
