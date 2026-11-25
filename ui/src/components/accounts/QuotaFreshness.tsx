import type { Account } from '../../types'
import { quotaIsFresh } from '../../utils/accountInsights'
import { useTimeValue } from '../../hooks/useSharedTicker'
import { EventTime } from './EventTime'

export function QuotaFreshness({ account, prefix = true }: { account: Account; prefix?: boolean }) {
  const fresh = useTimeValue(() => quotaIsFresh(account), Boolean(account.quota))
  if (!account.quota) return <span className="text-xs text-ag-muted">No quota data</span>
  return (
    <span className={`text-[11px] font-normal ${fresh ? 'text-ag-muted' : 'text-amber-400/90'}`}>
      {!fresh && 'Outdated · '}{fresh && prefix && 'Updated '}<EventTime timestamp={account.quota.fetchedAt} />
    </span>
  )
}
