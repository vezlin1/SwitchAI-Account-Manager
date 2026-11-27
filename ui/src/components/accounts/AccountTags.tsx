import { lazy, Suspense, useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { ArrowLeft, ChevronDown, Plus, Tag, Trash2, X } from 'lucide-react'
import type { Account, AccountTag } from '../../types'
import { useDialogFocus } from '../../hooks/useDialogFocus'
import { usePrivacy } from '../../context/usePrivacy'
import { describeIpcError } from '../../api'

const ConfirmDialog = lazy(() => import('../common/ConfirmDialog').then(module => ({ default: module.ConfirmDialog })))

type TagOptions = Pick<AccountTag, 'showInAll' | 'refreshOnlyInTab'>
const defaultOptions: TagOptions = { showInAll: true, refreshOnlyInTab: false }

function TagPreferences({ value, onChange }: {
  value: TagOptions
  onChange: (options: TagOptions) => void
}) {
  return (
    <fieldset className="tag-preferences">
      <legend className="visually-hidden">Tag settings</legend>
      <div className="tag-options">
        <label className="tag-option">
          <span>Show accounts in other tabs</span>
          <input type="checkbox" className="tag-check" checked={value.showInAll}
            onChange={e => onChange({ showInAll: e.target.checked, refreshOnlyInTab: value.refreshOnlyInTab })} />
        </label>
        <label className="tag-option">
          <span>Refresh only in this tag’s tab</span>
          <input type="checkbox" className="tag-check" checked={value.refreshOnlyInTab}
            onChange={e => onChange({ showInAll: value.showInAll, refreshOnlyInTab: e.target.checked })} />
        </label>
      </div>
    </fieldset>
  )
}

function TagEditorFrame({ title, accountLabel, busy, onClose, onBack, children }: {
  title: string
  accountLabel?: string
  busy: boolean
  onClose: () => void
  onBack?: () => void
  children: ReactNode
}) {
  const dialog = useRef<HTMLDivElement>(null)
  const titleId = useId()
  useDialogFocus(dialog, onBack ?? onClose, !busy)
  return (
    <div className="settings-backdrop" onClick={event => {
      if (event.target === event.currentTarget && !busy) onClose()
    }}>
      <div ref={dialog} className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1}>
        <div className="settings-dialog-header">
          <div className="tag-dialog-heading">
            <h2 id={titleId} className="settings-dialog-title"><Tag size={16} aria-hidden="true" />{title}</h2>
            {accountLabel && <p className="tag-account-email">{accountLabel}</p>}
          </div>
          {!onBack && <button type="button" className="settings-close" onClick={onClose} disabled={busy} aria-label="Close"><X size={16} /></button>}
        </div>
        {children}
      </div>
    </div>
  )
}

export type TagEditorTarget = { account: Account } | { tag: AccountTag }

export function AccountTagsDialog({ target, tags, onSave, onClose }: {
  target: TagEditorTarget
  tags: AccountTag[]
  onSave: (tags: AccountTag[]) => Promise<unknown>
  onClose: () => void
}) {
  const account = 'account' in target ? target.account : null
  const group = 'tag' in target ? target.tag : null
  const provider = account?.provider ?? group?.provider ?? 'codex'
  const choices = tags.filter(tag => tag.provider === provider)
  const [selected, setSelected] = useState(() => new Set(
    choices.filter(tag => account && tag.accountIds.includes(account.id)).map(tag => tag.id)
  ))
  const [name, setName] = useState(group?.name ?? '')
  const [options, setOptions] = useState<TagOptions>(group ? {
    showInAll: group.showInAll, refreshOnlyInTab: group.refreshOnlyInTab
  } : defaultOptions)
  const [creating, setCreating] = useState(false)
  const [confirmRemoval, setConfirmRemoval] = useState(false)
  const [error, setError] = useState('')
  const [nameError, setNameError] = useState('')
  const [busy, setBusy] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const pickerTrigger = useRef<HTMLButtonElement>(null)
  const nameInput = useRef<HTMLInputElement>(null)
  const pickerLabel = useId()
  const pickerPanelId = useId()
  const selectionLabel = useId()
  const nameId = useId()
  const nameErrorId = useId()
  const { privacyMode, maskEmail } = usePrivacy()
  const selectedNames = choices.filter(tag => selected.has(tag.id)).map(tag => tag.name)

  useEffect(() => {
    if (creating) nameInput.current?.focus()
    else pickerTrigger.current?.focus()
  }, [creating])

  const backToTags = () => {
    setCreating(false)
    setNameError('')
    setError('')
  }

  const toggleAssignment = (id: AccountTag['id'], checked: boolean) => {
    setSelected(previous => {
      const next = new Set(previous)
      if (checked) next.add(id)
      else next.delete(id)
      return next
    })
  }

  const commit = async (next: AccountTag[]) => {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await onSave(next)
      onClose()
    } catch (err) {
      setError(describeIpcError(err))
      setBusy(false)
    }
  }

  const save = (event: FormEvent) => {
    event.preventDefault()
    const label = name.trim()
    if (group || creating) {
      const duplicate = choices.some(tag => tag.id !== group?.id && tag.name.toLowerCase() === label.toLowerCase())
      if (!label || duplicate) {
        setNameError(duplicate ? 'A tag with this name already exists.' : 'Enter a tag name.')
        nameInput.current?.focus()
        return
      }
    }
    if (group) {
      void commit(tags.map(tag => tag.id === group.id ? { ...tag, name: label, ...options } : tag))
    } else if (account) {
      const next = tags.map(tag => {
        if (tag.provider !== provider || selected.has(tag.id) === tag.accountIds.includes(account.id)) return tag
        const accountIds = selected.has(tag.id)
          ? [...tag.accountIds, account.id]
          : tag.accountIds.filter(id => id !== account.id)
        return { ...tag, accountIds }
      })
      if (creating) next.push({ id: `tag:${crypto.randomUUID()}`, name: label, provider, accountIds: [account.id], ...options })
      void commit(next)
    }
  }

  if (group && confirmRemoval) {
    return createPortal(<Suspense fallback={null}><ConfirmDialog
      title={`Remove “${group.name}”?`}
      message="This removes the tag from all assigned accounts and deletes its tab."
      confirmLabel="Remove tag"
      variant="danger"
      busy={busy}
      error={error}
      onCancel={() => { setError(''); setConfirmRemoval(false) }}
      onConfirm={() => void commit(tags.filter(tag => tag.id !== group.id))}
    /></Suspense>, document.body)
  }

  return createPortal(
    <TagEditorFrame title={creating ? 'Create tag' : group ? 'Tag settings' : 'Account tags'}
      accountLabel={account ? (privacyMode ? maskEmail(account.email) : account.email ?? 'Unnamed account') : undefined}
      busy={busy} onClose={onClose} onBack={creating ? backToTags : undefined}>
      <form className="tag-form" onSubmit={save}>
        <div className="settings-dialog-body tag-editor-body">
          {account && !creating && (
            <div className="tag-field">
              <span id={pickerLabel} className="tag-field-label">Assigned tags</span>
              <div className="tag-picker"
                onKeyDown={e => {
                  if (e.key === 'Escape' && pickerOpen) {
                    e.stopPropagation()
                    e.preventDefault()
                    setPickerOpen(false)
                    pickerTrigger.current?.focus()
                  }
                }}>
                <button type="button" className="tag-picker-trigger" ref={pickerTrigger} data-autofocus
                  aria-labelledby={`${pickerLabel} ${selectionLabel}`} aria-expanded={pickerOpen}
                  aria-controls={pickerPanelId} onClick={() => setPickerOpen(open => !open)}>
                  <span id={selectionLabel} className="tag-picker-value">
                    {selectedNames.length
                      ? selectedNames.map(label => <span key={label} className="tag-selected" title={label}>{label}</span>)
                      : 'Choose tags'}
                  </span>
                  <ChevronDown size={16} aria-hidden="true" />
                </button>
                <div id={pickerPanelId} className="tag-picker-panel" inert={!pickerOpen}>
                  <div className="tag-picker-clip">
                    <div className="tag-picker-list">
                      {choices.map(tag => <label key={tag.id} className="tag-choice">
                        <input type="checkbox" className="tag-check" checked={selected.has(tag.id)}
                          onChange={e => toggleAssignment(tag.id, e.target.checked)} />
                        <span>{tag.name}</span>
                      </label>)}
                      <button type="button" className="tag-create" onClick={() => {
                        setError('')
                        setCreating(true)
                      }}><Plus size={16} aria-hidden="true" />Create new tag</button>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
          {(group || creating) && <div className="tag-edit-fields">
            <div className="tag-field">
              <label className="tag-field-label" htmlFor={nameId}>Tag name</label>
              <input id={nameId} name="tagName" ref={nameInput} className="tag-name" data-autofocus={group ? true : undefined}
                maxLength={80} required value={name} autoComplete="off" aria-invalid={nameError ? true : undefined}
                aria-describedby={nameError ? nameErrorId : undefined}
                onChange={e => { setName(e.target.value); setNameError('') }} />
              {nameError && <p className="settings-error" id={nameErrorId} role="alert">{nameError}</p>}
            </div>
            <TagPreferences value={options} onChange={setOptions} />
          </div>}
          {error && <p className="settings-error" role="alert">{error}</p>}
        </div>
        <div className="settings-dialog-footer">
          {group && <button type="button" className="settings-cancel tag-delete" disabled={busy}
            onClick={() => { setError(''); setConfirmRemoval(true) }}>
            <Trash2 size={14} aria-hidden="true" />Remove tag
          </button>}
          {creating
            ? <button type="button" className="tag-back" onClick={backToTags} disabled={busy}>
              <ArrowLeft size={15} aria-hidden="true" />Back
            </button>
            : <button type="button" className="settings-cancel" onClick={onClose} disabled={busy}>Cancel</button>}
          <button type="submit" className="settings-save" disabled={busy}>{creating ? 'Create and assign' : 'Save'}</button>
        </div>
      </form>
    </TagEditorFrame>, document.body
  )
}
