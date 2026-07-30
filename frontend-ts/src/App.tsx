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
import logo from './assets/logo.svg'
import { fetchWithRetry, FetchError } from './lib/fetchWithRetry'

type AppState = 'input' | 'rooftop' | 'map'

interface ActiveMap {
  analysisId: string
  overview: AnalysisOverview
}

// ── API helpers ───────────────────────────────────────────────────────────────

async function toNys(lat: number, lon: number, alt_m: number): Promise<[number, number, number]> {
  const res = await fetchWithRetry('/api/coords/toNys', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ gps: [lat, lon, alt_m] }),
  })
  const d = await res.json() as { nys: [number, number, number] }
  return d.nys
}

interface SamplePointsResponse {
  sample_points: BackendSamplePoint[]
  sw_offset: [number, number]
}

async function getSamplePoints(
  binId: string,
  params: { mast_offset_ft: number; sample_spacing?: number },
): Promise<SamplePointsResponse> {
  const body: Record<string, unknown> = { mast_offset_ft: params.mast_offset_ft }
  if (params.sample_spacing !== undefined) body.sample_spacing = params.sample_spacing
  const res = await fetchWithRetry(`/api/rooftop/samplePoints/${binId}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  return res.json() as Promise<SamplePointsResponse>
}

// isAborted is polled by fetchWithRetry so cancellation (the "Stop" button, or navigating away)
// interrupts an in-flight retry/backoff promptly instead of running to completion first.
async function analyzePoint(
  pt: BackendSamplePoint,
  nysB: [number, number, number],
  freqGhz: number,
  isAborted: () => boolean = () => false,
): Promise<PointAnalysis> {
  let res: Response
  try {
    res = await fetchWithRetry('/api/analysis/analyzePointPair', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        point_a_nys: pt.sample_point.nys,
        point_b_nys: nysB,
        frequency_hz: freqGhz * 1e9,
      }),
    }, isAborted)
  } catch (err) {
    // 413 means this link/frequency would never fit in the server's memory budget —
    // fetchWithRetry already knows not to retry it, but give it a clearer message here.
    if (err instanceof FetchError && err.status === 413) {
      throw new FetchError('This link is too long (or frequency too low) to analyze', false, 413)
    }
    throw err
  }
  const d = await res.json() as { id: string; result: string }
  const resultMap: Record<string, string> = {
    Unobstructed:        'unobstructed',
    PartiallyObstructed: 'partially_obstructed',
    Obstructed:          'obstructed',
  }
  return { id: d.id, result: resultMap[d.result] ?? d.result.toLowerCase() } as PointAnalysis
}

function toErrorAnalysis(err: unknown): PointAnalysis {
  if (err instanceof FetchError && !err.retryable)
    return { id: '', result: 'error_fatal', errorMessage: err.message }
  return { id: '', result: 'error' }
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
  const [binId,         setBinId]         = useState<string | null>(null)
  const [buildingLabel, setBuildingLabel] = useState<string | null>(null)
  const [samplePoints, setSamplePoints] = useState<BackendSamplePoint[]>([])
  const [analyses,     setAnalyses]     = useState<(PointAnalysis | null | undefined)[]>([])
  const [nysB,         setNysB]         = useState<[number, number, number] | null>(null)
  const [freqGhz,        setFreqGhz]        = useState(24)
  const [mastOffsetFt,   setMastOffsetFt]   = useState(4)
  const [buildingOffset, setBuildingOffset] = useState<{ x_sw: number; y_sw: number } | null>(null)

  // Map state
  const [activeMap, setActiveMap] = useState<ActiveMap | null>(null)

  const [errorPopup, setErrorPopup] = useState<string | null>(null)

  // Abort ref — set to true to stop the analysis loop between requests
  const abortRef = useRef(false)
  const [analyzing, setAnalyzing] = useState(false)

  // Stops any in-flight bulk analysis. Must be called both from the explicit "Stop" button and
  // from any navigation away from the rooftop view — otherwise the analysis loop (and its
  // still-pending handleSubmit promise, which InputForm's submit button stays disabled until it
  // resolves) keeps running invisibly in the background after the user navigates away.
  const cancelAnalysis = useCallback(() => {
    abortRef.current = true
    setAnalyzing(false)
    setLoading(null)
  }, [])

  // Persists rooftop camera across map view navigation
  const rooftopCameraRef = useRef<RooftopCameraState | null>(null)

  const handleSubmit = useCallback(async (values: RooftopSubmitValues) => {
    // Reset
    setBinId(null); setBuildingLabel(null); setSamplePoints([]); setAnalyses([]); setActiveMap(null); setNysB(null); setBuildingOffset(null)
    setFreqGhz(values.frequency_ghz)
    setMastOffsetFt(values.mast_offset_ft)
    setAppState('rooftop')

    try {
      let nysBPoint: [number, number, number]
      if (values.far_end_nys) {
        nysBPoint = values.far_end_nys
      } else {
        setLoading({ message: 'Converting far-end coordinates…' })
        nysBPoint = await toNys(values.lat!, values.lon!, values.alt_m!)
      }

      setLoading({ message: 'Loading rooftop sample points…' })
      const { sample_points: points, sw_offset } = await getSamplePoints(values.bin_id, {
        mast_offset_ft: values.mast_offset_ft,
        sample_spacing: values.sample_spacing,
      })

      setBinId(values.bin_id)
      setBuildingLabel(values.building_label ?? null)
      setNysB(nysBPoint)
      setSamplePoints(points)
      setBuildingOffset({ x_sw: sw_offset[0], y_sw: sw_offset[1] })

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
          const result = await analyzePoint(points[ptIdx], nysBPoint, values.frequency_ghz, () => abortRef.current)
          if (abortRef.current) return
          setAnalyses(prev => { const next = [...prev]; next[ptIdx] = result; return next })
        } catch (err) {
          if (!abortRef.current)
            setAnalyses(prev => { const next = [...prev]; next[ptIdx] = toErrorAnalysis(err); return next })
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

    if (analysis === undefined || analysis?.result === 'error') {
      // Not yet requested, or retryable error — retry immediately, jumping the queue
      if (!nysB) return
      setAnalyses(prev => { const next = [...prev]; next[idx] = null; return next })
      try {
        const result = await analyzePoint(samplePoints[idx], nysB, freqGhz, () => false)
        setAnalyses(prev => { const next = [...prev]; next[idx] = result; return next })
      } catch (err) {
        setAnalyses(prev => { const next = [...prev]; next[idx] = toErrorAnalysis(err); return next })
      }
      return
    }

    if (analysis === null) return  // already in flight

    if (analysis.result === 'error_fatal') {
      setErrorPopup(analysis.errorMessage ?? 'Analysis failed')
      return
    }

    setActiveMap(null)
    setAppState('map')
    setLoading({ message: 'Loading map overview…' })

    try {
      const res = await fetchWithRetry(`/api/analysis/overview/${analysis.id}`, { method: 'POST' })
      const overview = await res.json() as AnalysisOverview
      setActiveMap({ analysisId: analysis.id, overview })
      setLoading(null)
    } catch (err) {
      setLoading({ message: String(err), isError: true })
    }
  }, [analyses, samplePoints, nysB, freqGhz])

  const handleAddCustomPoint = useCallback(async (point: BackendSamplePoint) => {
    if (!nysB) return
    const newIdx = samplePoints.length
    setSamplePoints(prev => [...prev, point])
    setAnalyses(prev => [...prev, null])
    try {
      const result = await analyzePoint(point, nysB, freqGhz, () => false)
      setAnalyses(prev => { const next = [...prev]; next[newIdx] = result; return next })
    } catch (err) {
      setAnalyses(prev => { const next = [...prev]; next[newIdx] = toErrorAnalysis(err); return next })
    }
  }, [samplePoints, nysB, freqGhz])

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <LoadingToast
        loading={loading}
        onAbort={analyzing ? cancelAnalysis : undefined}
      />

      <div style={{ display: appState === 'input' ? 'flex' : 'none', flex: 1 }}>
        <InputForm onSubmit={handleSubmit} />
      </div>

      {appState === 'rooftop' && (
        <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
          <TopBar
            left={<>
              <img src={logo} alt="LOScope" style={styles.barLogo} />
              <BackButton onClick={() => { cancelAnalysis(); setAppState('input') }} />
            </>}
            center={binId && <RooftopHUD binId={binId} label={buildingLabel} />}
            right={<Hint>Click a point to view tile map</Hint>}
          />
          {binId && buildingOffset !== null ? (
            <RooftopViewer
              binId={binId}
              samplePoints={samplePoints}
              analyses={analyses}
              cameraStateRef={rooftopCameraRef}
              onPointClick={handlePointClick}
              nysB={nysB}
              mastOffsetFt={mastOffsetFt}
              buildingOffset={buildingOffset}
              onAddCustomPoint={handleAddCustomPoint}
            />
          ) : (
            <WaitingScreen label={
              loading?.isError ? `Error: ${loading.message}` : 'Loading rooftop…'
            } />
          )}
          {errorPopup !== null && (
            <div style={styles.popupBackdrop}>
              <div style={styles.errorPopup}>
                <div style={styles.popupTitle}>Analysis failed</div>
                <div style={styles.popupMessage}>{errorPopup}</div>
                <button style={styles.popupDismiss} onClick={() => setErrorPopup(null)}>Dismiss</button>
              </div>
            </div>
          )}
        </div>
      )}

      {appState === 'map' && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={styles.topBarFlat}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <img src={logo} alt="LOScope" style={styles.barLogo} />
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
      const res = await fetchWithRetry(`/api/analysis/fresnelKml/${analysisId}`, { method: 'POST' })
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

function RooftopHUD({ binId, label }: { binId: string; label: string | null }) {
  const showLabel = label && label !== `BIN ${binId}`
  return (
    <span style={styles.hud}>
      {showLabel ? <>{label}&nbsp;·&nbsp;</> : null}BIN {binId}
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
  popupBackdrop: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 200,
  },
  errorPopup: {
    background: 'rgba(13, 17, 23, 0.97)',
    border: '1px solid rgba(255, 68, 68, 0.4)',
    borderRadius: 8,
    padding: '18px 22px',
    fontFamily: 'monospace',
    fontSize: 12,
    maxWidth: 320,
    boxShadow: '0 8px 32px rgba(0,0,0,0.6)',
  },
  popupTitle: {
    color: '#ff4444',
    fontWeight: 600,
    marginBottom: 8,
  },
  popupMessage: {
    color: '#8b949e',
    marginBottom: 16,
    lineHeight: 1.5,
  },
  popupDismiss: {
    background: 'rgba(255,255,255,0.06)',
    border: '1px solid rgba(255,255,255,0.12)',
    borderRadius: 4,
    color: '#8b949e',
    fontSize: 12,
    fontFamily: 'monospace',
    padding: '3px 12px',
    cursor: 'pointer',
  },
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
  barLogo: {
    height: 22,
    display: 'block',
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
