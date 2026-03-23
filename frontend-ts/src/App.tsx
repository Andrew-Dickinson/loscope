import { useState, useCallback } from 'react'
import type { ReactNode } from 'react'
import InputForm from './components/InputForm/InputForm'
import type { RooftopSubmitValues } from './components/InputForm/InputForm'
import RooftopViewer from './components/RooftopViewer/RooftopViewer'
import type { RooftopResult, SamplePoint } from './components/RooftopViewer/RooftopViewer'
import TileMap from './components/TileMap/TileMap'
import LoadingToast from './components/ui/LoadingToast'
import { useJob } from './hooks/useJob'
import type { JobState } from './hooks/useJob'

type AppState = 'input' | 'rooftop' | 'tilemap'

export default function App() {
  const [appState, setAppState]         = useState<AppState>('input')
  const [rooftopJobId, setRooftopJobId] = useState<string | null>(null)
  const [tileMapJobId, setTileMapJobId] = useState<string | null>(null)
  const [freqGhz, setFreqGhz]           = useState(24)

  const rooftopJob  = useJob(rooftopJobId)
  const tileMapJob  = useJob(tileMapJobId)

  // Show toast for whichever job is currently running
  const activeJob: JobState | null = (() => {
    if (tileMapJob && tileMapJob.status !== 'done' && tileMapJob.status !== 'error') return tileMapJob
    if (rooftopJob && rooftopJob.status !== 'done' && rooftopJob.status !== 'error') return rooftopJob
    return null
  })()

  const rooftopResult: RooftopResult | null =
    rooftopJob?.status === 'done' ? (rooftopJob.result as unknown as RooftopResult) : null

  // ── Submit rooftop evaluation ─────────────────────────────────────────────
  const handleRooftopSubmit = useCallback(async (values: RooftopSubmitValues) => {
    setRooftopJobId(null)
    setTileMapJobId(null)
    setFreqGhz(values.frequency_ghz)
    setAppState('rooftop')
    try {
      const res = await fetch('/api/evaluate-rooftop', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(values),
      })
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const { job_id } = await res.json() as { job_id: string }
      setRooftopJobId(job_id)
    } catch (err) {
      console.error('Rooftop submit failed:', err)
      setAppState('input')
    }
  }, [])

  // ── Point click → tile map ────────────────────────────────────────────────
  const handlePointClick = useCallback(async (point: SamplePoint) => {
    if (!rooftopResult?._nys_b) return
    setTileMapJobId(null)
    setAppState('tilemap')
    try {
      const res = await fetch('/api/tile-map', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          nys_a: [point.nys_e, point.nys_n, point.nys_z],
          nys_b: rooftopResult._nys_b,
          frequency_ghz: freqGhz,
        }),
      })
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const { job_id } = await res.json() as { job_id: string }
      setTileMapJobId(job_id)
    } catch (err) {
      console.error('Tile-map submit failed:', err)
    }
  }, [rooftopResult, freqGhz])

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <LoadingToast job={activeJob} />

      {appState === 'input' && (
        <InputForm onSubmit={handleRooftopSubmit} />
      )}

      {appState === 'rooftop' && (
        <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
          <TopBar
            left={<BackButton onClick={() => setAppState('input')} />}
            center={rooftopResult && <RooftopHUD result={rooftopResult} />}
            right={<Hint>Click a point to view tile map</Hint>}
          />
          {rooftopResult ? (
            <RooftopViewer
              jobId={rooftopJobId}
              result={rooftopResult}
              onPointClick={handlePointClick}
            />
          ) : (
            <WaitingScreen job={rooftopJob} label="Evaluating building…" />
          )}
        </div>
      )}

      {appState === 'tilemap' && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={styles.topBarFlat}>
            <BackButton onClick={() => setAppState('rooftop')} />
            <Hint>Click a tile to open 3D view</Hint>
          </div>
          <div style={{ flex: 1, overflow: 'hidden' }}>
            <TileMap
              tileMapJob={tileMapJob}
              rooftopResult={rooftopResult}
              frequency_ghz={freqGhz}
            />
          </div>
        </div>
      )}
    </div>
  )
}

// ── Shared UI helpers ─────────────────────────────────────────────────────────

interface TopBarProps {
  left: ReactNode
  center: ReactNode
  right: ReactNode
}

function TopBar({ left, center, right }: TopBarProps) {
  return (
    <div style={styles.topBarAbs}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        {left}
        {center}
      </div>
      <div>{right}</div>
    </div>
  )
}

function BackButton({ onClick }: { onClick: () => void }) {
  return (
    <button style={styles.backBtn} onClick={onClick}>← Back</button>
  )
}

function Hint({ children }: { children: ReactNode }) {
  return <span style={styles.hint}>{children}</span>
}

function RooftopHUD({ result }: { result: RooftopResult }) {
  const { n_clear, n_partial, n_full } = result.summary
  return (
    <span style={styles.hud}>
      BIN {result.bin_id}&nbsp;·&nbsp;
      <span style={{ color: '#22cc44' }}>{n_clear} clear</span>&nbsp;·&nbsp;
      <span style={{ color: '#ffcc00' }}>{n_partial} partial</span>&nbsp;·&nbsp;
      <span style={{ color: '#ff4444' }}>{n_full} blocked</span>
    </span>
  )
}

function WaitingScreen({ job, label }: { job: JobState | null; label: string }) {
  return (
    <div style={styles.waiting}>
      {job?.status === 'error'
        ? <span style={{ color: '#ff4444' }}>Error: {job.error?.trim().split('\n').pop()}</span>
        : <span style={{ color: '#484f58' }}>{label}</span>}
    </div>
  )
}

const BAR_H = 42
const styles: Record<string, React.CSSProperties> = {
  topBarAbs: {
    position: 'absolute',
    top: 0, left: 0, right: 0,
    zIndex: 100,
    height: BAR_H,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '0 14px',
    background: 'rgba(13,17,23,0.88)',
    backdropFilter: 'blur(6px)',
    borderBottom: '1px solid rgba(255,255,255,0.07)',
  },
  topBarFlat: {
    flexShrink: 0,
    height: BAR_H,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '0 14px',
    background: 'rgba(13,17,23,0.95)',
    borderBottom: '1px solid rgba(255,255,255,0.07)',
  },
  backBtn: {
    color: '#8b949e',
    fontSize: 12,
    fontFamily: 'monospace',
    padding: '3px 10px',
    borderRadius: 4,
    border: '1px solid rgba(255,255,255,0.1)',
    background: 'rgba(255,255,255,0.04)',
    cursor: 'pointer',
  },
  hud: {
    color: '#8b949e',
    fontSize: 12,
    fontFamily: 'monospace',
    userSelect: 'none',
  },
  hint: {
    color: '#3d444d',
    fontSize: 12,
    fontFamily: 'monospace',
    userSelect: 'none',
  },
  waiting: {
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: 13,
    fontFamily: 'monospace',
  },
}
