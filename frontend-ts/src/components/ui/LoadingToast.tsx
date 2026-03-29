export interface LoadingState {
  message: string
  progress?: number
  isError?: boolean
}

interface LoadingToastProps {
  loading: LoadingState | null
}

/**
 * Non-intrusive loading indicator — fixed top-center bar.
 */
export default function LoadingToast({ loading }: LoadingToastProps) {
  if (!loading) return null

  const { message, progress, isError = false } = loading

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
      {!isError && (
        <div style={{ height: 2, background: 'rgba(255,255,255,0.08)' }}>
          <div style={{
            height: '100%',
            width: progress != null && progress > 0 ? `${progress}%` : '30%',
            background: 'linear-gradient(90deg, #388bfd, #58a6ff)',
            transition: progress != null && progress > 0 ? 'width 0.4s ease' : undefined,
            animation: progress == null || progress === 0 ? 'indeterminate 1.4s ease infinite' : undefined,
          }} />
        </div>
      )}
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
          {message}
        </span>
        {progress != null && progress > 0 && !isError && (
          <span style={{ color: '#484f58', flexShrink: 0 }}>{Math.round(progress)}%</span>
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
