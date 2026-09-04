import { Suspense, lazy, useEffect, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '../../api'
import type { UpdateCheckResult } from '../../types'

const UpdateModal = lazy(() =>
  import('../modals/UpdateModal').then((m) => ({ default: m.UpdateModal }))
)

export type UpdateCoordinatorProps = {
  updateModalOpen: boolean
  setUpdateModalOpen: (open: boolean) => void
  updateInfo: UpdateCheckResult | null
  setUpdateInfo: (info: UpdateCheckResult | null) => void
}

export function UpdateCoordinator({
  updateModalOpen,
  setUpdateModalOpen,
  updateInfo,
  setUpdateInfo
}: UpdateCoordinatorProps) {
  const updateInfoRef = useRef<UpdateCheckResult | null>(null)

  useEffect(() => {
    updateInfoRef.current = updateInfo
  }, [updateInfo])

  useEffect(() => {
    let disposed = false
    let unlistenAvailable: UnlistenFn | undefined
    let unlistenOpenModal: UnlistenFn | undefined

    void listen<UpdateCheckResult>('update-available', (event) => {
      setUpdateInfo(event.payload)
    }).then((fn) => {
      if (disposed) fn()
      else unlistenAvailable = fn
    })

    void listen('open-update-modal', () => {
      if (updateInfoRef.current) {
        setUpdateModalOpen(true)
      } else {
        void api.checkForUpdates(true).then((res) => {
          setUpdateInfo(res)
          if (res.updateAvailable) {
            setUpdateModalOpen(true)
          }
        })
      }
    }).then((fn) => {
      if (disposed) fn()
      else unlistenOpenModal = fn
    })

    return () => {
      disposed = true
      unlistenAvailable?.()
      unlistenOpenModal?.()
    }
  }, [setUpdateInfo, setUpdateModalOpen])

  if (!updateModalOpen || !updateInfo) {
    return null
  }

  return (
    <Suspense fallback={null}>
      <UpdateModal
        isOpen={updateModalOpen}
        onClose={() => setUpdateModalOpen(false)}
        updateInfo={updateInfo}
        onDismissVersion={async (ver) => {
          await api.dismissUpdateVersion(ver)
          setUpdateInfo(null)
        }}
      />
    </Suspense>
  )
}
