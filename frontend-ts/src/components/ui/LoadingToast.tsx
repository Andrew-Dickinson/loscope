import { type JobState } from '../../hooks/useJob'

interface LoadingToastProps {
  job: JobState | null
}

/**
 * Non-intrusive loading indicator — fixed top-center bar.
 * Shows when a job is running, disappears when done or on error.
 */
export default function LoadingToast({ job }: LoadingToastProps) {
  if (!job || job.status === 'done') return null

  const isError = job.status === 'error'
  const pct = job.progress_pct ?? 0
  const msg = job.message || (isError ? 'Error' : 'Processing…')

  return (
    <div style={{
      position: 'fixed',
      top: 0,
      left: '50%',
      transform: 'translateX(-50%)',
      zIndex: 9999,
      minWidth: 280,
      maxWidth: 420,
      background: isError ? 'rgba(255,68,68,0.12)' : 'rgba(22,27,34,0.96)',
      border: `1px solid ${isError ? 'rgba(255,68,68,0.4)' : 'rgba(255,255,255,0.1)'}`,
      borderRadius: '0 0 8px 8px',
      overflow: 'hidden',
      boxShadow: '0 4px 24px rgba(0,0,0,0.5)',
      backdropFilter: 'blur(8px)',
    }}>
      {/* Progress bar */}
      {!isError && (
        <div style={{
          height: 2,
          background: 'rgba(255,255,255,0.08)',
        }}>
          <div style={{
            height: '100%',
            width: pct > 0 ? `${pct}%` : '30%',
            background: 'linear-gradient(90deg, #388bfd, #58a6ff)',
            transition: pct > 0 ? 'width 0.4s ease' : undefined,
            animation: pct === 0 ? 'indeterminate 1.4s ease infinite' : undefined,
          }} />
        </div>
      )}
      {/* Message */}
      <div style={{
        padding: '6px 14px',
        fontSize: 12,
        fontFamily: 'monospace',
        color: isError ? '#ff4444' : '#8b949e',
        display: 'flex',
        alignItems: 'center',
        gap: 8,
      }}>
        {!isError && (
          <span style={{
            display: 'inline-block',
            width: 7,
            height: 7,
            borderRadius: '50%',
            background: '#388bfd',
            animation: 'pulse 1.2s ease infinite',
          }} />
        )}
        <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {msg}
        </span>
        {pct > 0 && !isError && (
          <span style={{ color: '#484f58', flexShrink: 0 }}>{pct}%</span>
        )}
      </div>
      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.3; }
        }
        @keyframes indeterminate {
          0% { transform: translateX(-100%); width: 40%; }
          100% { transform: translateX(350%); width: 40%; }
        }
      `}</style>
    </div>
  )
}
