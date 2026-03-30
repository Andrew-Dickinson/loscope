import { useState, useCallback, useRef } from 'react'
import type { ReactNode } from 'react'
import InputForm from './components/InputForm/InputForm'
import type { RooftopSubmitValues } from './components/InputForm/InputForm'
import RooftopViewer from './components/RooftopViewer/RooftopViewer'
import type { BackendSamplePoint, PointAnalysis, RooftopCameraState } from './components/RooftopViewer/RooftopViewer'
import TileMap from './components/TileMap/TileMap'
import type { AnalysisOverview } from './components/TileMap/TileMap'
import LoadingToast from './components/ui/LoadingToast'
import type { LoadingState } from './components/ui/LoadingToast'

type AppState = 'input' | 'rooftop' | 'map'

interface ActiveMap {
  analysisId: string
  overview: AnalysisOverview
}

// ── API helpers ───────────────────────────────────────────────────────────────

async function toNys(lat: number, lon: number, alt_m: number): Promise<[number, number, number]> {
  const res = await fetch('/api/coords/toNys', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ lat, lon, alt_m }),
  })
  if (!res.ok) throw new Error(`Coordinate conversion failed: HTTP ${res.status}`)
  const d = await res.json() as { nys_e: number; nys_n: number; nys_z: number }
  return [d.nys_e, d.nys_n, d.nys_z]
}

async function getSamplePoints(
  binId: string,
  params: { mast_offset_ft: number; sample_spacing: number },
): Promise<BackendSamplePoint[]> {
  const res = await fetch(`/api/rooftop/samplePoints/${binId}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(params),
  })
  if (!res.ok) throw new Error(`Sample points failed: HTTP ${res.status}`)
  const data = await res.json() as { sample_points: BackendSamplePoint[] }
  return data.sample_points
}

async function analyzePoint(
  pt: BackendSamplePoint,
  nysB: [number, number, number],
  freqGhz: number,
): Promise<PointAnalysis> {
  const res = await fetch('/api/analysis/analyzePointPair', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      point_a_nys: [pt.measurement_point.nys_e, pt.measurement_point.nys_n, pt.measurement_point.nys_z],
      point_b_nys: nysB,
      frequency_ghz: freqGhz,
    }),
  })
  if (!res.ok) throw new Error(`Analysis failed: HTTP ${res.status}`)
  return res.json() as Promise<PointAnalysis>
}

async function runConcurrent<T>(
  items: T[],
  fn: (item: T, idx: number) => Promise<void>,
  concurrency = 4,
): Promise<void> {
  let next = 0
  const worker = async () => {
    while (next < items.length) {
      const i = next++
      await fn(items[i], i)
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker))
}

// ── App ───────────────────────────────────────────────────────────────────────

export default function App() {
  const [appState, setAppState] = useState<AppState>('input')
  const [loading,  setLoading]  = useState<LoadingState | null>(null)

  // Rooftop state
  const [binId,        setBinId]        = useState<string | null>(null)
  const [samplePoints, setSamplePoints] = useState<BackendSamplePoint[]>([])
  const [analyses,     setAnalyses]     = useState<(PointAnalysis | null | undefined)[]>([])
  const [nysB,         setNysB]         = useState<[number, number, number] | null>(null)
  const [freqGhz,      setFreqGhz]      = useState(24)

  // Map state
  const [activeMap, setActiveMap] = useState<ActiveMap | null>(null)

  // Abort ref — set to true to stop the analysis loop between requests
  const abortRef = useRef(false)
  const [analyzing, setAnalyzing] = useState(false)

  // Persists rooftop camera across map view navigation
  const rooftopCameraRef = useRef<RooftopCameraState | null>(null)

  const handleSubmit = useCallback(async (values: RooftopSubmitValues) => {
    // Reset
    setBinId(null); setSamplePoints([]); setAnalyses([]); setActiveMap(null); setNysB(null)
    setFreqGhz(values.frequency_ghz)
    setAppState('rooftop')

    try {
      setLoading({ message: 'Converting far-end coordinates…' })
      const nysBPoint = await toNys(values.lat, values.lon, values.alt_m)

      setLoading({ message: 'Loading rooftop sample points…' })
      const points = await getSamplePoints(values.bin_id, {
        mast_offset_ft: values.mast_offset_ft,
        sample_spacing: values.sample_spacing,
      })

      setBinId(values.bin_id)
      setNysB(nysBPoint)
      setSamplePoints(points)

      if (points.length === 0) {
        setLoading(null)
        return
      }

      const total = points.length
      let done = 0
      setAnalyses(new Array(total).fill(undefined))
      setLoading({ message: `Analyzing 0 / ${total} points…`, progress: 0 })

      const indices = Array.from({ length: total }, (_, i) => i)
      for (let i = indices.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        ;[indices[i], indices[j]] = [indices[j], indices[i]]
      }

      abortRef.current = false
      setAnalyzing(true)
      await runConcurrent(indices, async (ptIdx) => {
        if (abortRef.current) return
        setAnalyses(prev => { const next = [...prev]; next[ptIdx] = null; return next })
        try {
          const result = await analyzePoint(points[ptIdx], nysBPoint, values.frequency_ghz)
          if (abortRef.current) return
          setAnalyses(prev => { const next = [...prev]; next[ptIdx] = result; return next })
        } catch {
          // Leave analysis[ptIdx] as null on error; counting as done anyway
        }
        done++
        setLoading({
          message: `Analyzing ${done} / ${total} points…`,
          progress: Math.round(done / total * 100),
        })
      })
      setAnalyzing(false)

      setLoading(null)
    } catch (err) {
      setLoading({ message: String(err), isError: true })
    }
  }, [])

  const handlePointClick = useCallback(async (idx: number) => {
    const analysis = analyses[idx]

    if (analysis === undefined) {
      // Not yet requested — fire immediately, jumping the queue
      if (!nysB) return
      setAnalyses(prev => { const next = [...prev]; next[idx] = null; return next })
      try {
        const result = await analyzePoint(samplePoints[idx], nysB, freqGhz)
        setAnalyses(prev => { const next = [...prev]; next[idx] = result; return next })
      } catch {
        setAnalyses(prev => { const next = [...prev]; next[idx] = undefined; return next })
      }
      return
    }

    if (analysis === null) return  // already in flight

    setActiveMap(null)
    setAppState('map')
    setLoading({ message: 'Loading map overview…' })

    try {
      const res = await fetch(`/api/analysis/overview/${analysis.analysis_id}`)
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const overview = await res.json() as AnalysisOverview
      setActiveMap({ analysisId: analysis.analysis_id, overview })
      setLoading(null)
    } catch (err) {
      setLoading({ message: String(err), isError: true })
    }
  }, [analyses, samplePoints, nysB, freqGhz])

  const n_clear   = analyses.filter(a => a?.result === 'unobstructed').length
  const n_partial = analyses.filter(a => a?.result === 'partially_obstructed').length
  const n_full    = analyses.filter(a => a?.result === 'obstructed').length

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <LoadingToast
        loading={loading}
        onAbort={analyzing ? () => { abortRef.current = true; setAnalyzing(false); setLoading(null) } : undefined}
      />

      {appState === 'input' && (
        <InputForm onSubmit={handleSubmit} />
      )}

      {appState === 'rooftop' && (
        <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
          <TopBar
            left={<BackButton onClick={() => { setAppState('input'); setLoading(null) }} />}
            center={binId && <RooftopHUD binId={binId} nClear={n_clear} nPartial={n_partial} nFull={n_full} />}
            right={<Hint>Click a point to view tile map</Hint>}
          />
          {binId && samplePoints.length > 0 ? (
            <RooftopViewer
              binId={binId}
              samplePoints={samplePoints}
              analyses={analyses}
              cameraStateRef={rooftopCameraRef}
              onPointClick={handlePointClick}
              nysB={nysB}
            />
          ) : (
            <WaitingScreen label={
              loading?.isError ? `Error: ${loading.message}` : 'Loading rooftop…'
            } />
          )}
        </div>
      )}

      {appState === 'map' && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={styles.topBarFlat}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <BackButton onClick={() => { setAppState('rooftop'); setLoading(null) }} />
              {activeMap && <KmlDownloadButton analysisId={activeMap.analysisId} />}
            </div>
            <Hint>Click a tile to open 3D view</Hint>
          </div>
          <div style={{ flex: 1, overflow: 'hidden' }}>
            {activeMap ? (
              <TileMap overview={activeMap.overview} analysisId={activeMap.analysisId} />
            ) : (
              <WaitingScreen label={
                loading?.isError ? `Error: ${loading.message}` : 'Loading map…'
              } />
            )}
          </div>
        </div>
      )}
    </div>
  )
}

// ── Shared UI helpers ─────────────────────────────────────────────────────────

function KmlDownloadButton({ analysisId }: { analysisId: string }) {
  const [loading, setLoading] = useState(false)

  const handleClick = async () => {
    if (loading) return
    setLoading(true)
    try {
      const res = await fetch(`/api/analysis/fresnelKml/${analysisId}`)
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const blob = await res.blob()
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `fresnel_${analysisId}.kml`
      a.click()
      URL.revokeObjectURL(url)
    } finally {
      setLoading(false)
    }
  }

  return (
    <>
      <style>{`@keyframes app-spin { to { transform: rotate(360deg); } }`}</style>
      <button style={{ ...styles.kmlBtn, opacity: loading ? 0.6 : 1, cursor: loading ? 'default' : 'pointer' }} onClick={handleClick} disabled={loading}>
        {loading
          ? <><span style={styles.kmlSpinner} />Generating…</>
          : 'Download KML'}
      </button>
    </>
  )
}

function TopBar({ left, center, right }: { left: ReactNode; center: ReactNode; right: ReactNode }) {
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
  return <button style={styles.backBtn} onClick={onClick}>← Back</button>
}

function Hint({ children }: { children: ReactNode }) {
  return <span style={styles.hint}>{children}</span>
}

function RooftopHUD({ binId, nClear, nPartial, nFull }: {
  binId: string; nClear: number; nPartial: number; nFull: number
}) {
  return (
    <span style={styles.hud}>
      BIN {binId}&nbsp;·&nbsp;
      <span style={{ color: '#22cc44' }}>{nClear} clear</span>&nbsp;·&nbsp;
      <span style={{ color: '#ffcc00' }}>{nPartial} partial</span>&nbsp;·&nbsp;
      <span style={{ color: '#ff4444' }}>{nFull} obstructed</span>
    </span>
  )
}

function WaitingScreen({ label }: { label: string }) {
  return (
    <div style={styles.waiting}>
      <span style={{ color: '#484f58' }}>{label}</span>
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
  kmlBtn: {
    color: '#8b949e',
    fontSize: 12,
    fontFamily: 'monospace',
    padding: '3px 10px',
    borderRadius: 4,
    border: '1px solid rgba(255,255,255,0.1)',
    background: 'rgba(255,255,255,0.04)',
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
  },
  kmlSpinner: {
    display: 'inline-block',
    width: 10,
    height: 10,
    borderRadius: '50%',
    border: '1.5px solid rgba(255,255,255,0.12)',
    borderTopColor: '#8b949e',
    animation: 'app-spin 0.7s linear infinite',
    flexShrink: 0,
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
