import { Suspense, lazy } from 'react'
import type { RecoveryStatus, StartupStatus } from '../../types'

const RecoveryModal = lazy(() =>
  import('../modals/RecoveryModal').then((m) => ({ default: m.RecoveryModal }))
)

export type StartupRecoveryGateProps = {
  startup: StartupStatus | null
  recovery: RecoveryStatus | null
  loading: boolean
  onRestore: () => Promise<unknown>
  onStartFresh: () => Promise<unknown>
  onOpenDataDirectory: () => Promise<unknown>
}

export function StartupRecoveryGate({
  startup,
  recovery,
  loading,
  onRestore,
  onStartFresh,
  onOpenDataDirectory
}: StartupRecoveryGateProps) {
  if (startup?.mode !== 'recovery_required' || !recovery) {
    return null
  }

  return (
    <Suspense fallback={null}>
      <RecoveryModal
        recovery={recovery}
        loading={loading}
        onRestore={onRestore}
        onStartFresh={onStartFresh}
        onOpenDataDirectory={onOpenDataDirectory}
      />
    </Suspense>
  )
}
