/**
 * 3D tile viewer using React Three Fiber.
 *
 * Fetches its own data directly from tileview endpoints — no job polling.
 *
 * - Heightmap: GET /api/tileview/terrain/heightRaster/<tileId> → TIFF decoded by geotiff.js
 * - Ortho texture: GET /api/tileview/terrain/orthoImage/<tileId>
 * - Obstruction OBJs: GET /api/tileview/terrain/obstructionObj/<type>/<id>/<tileId>
 * - Fresnel zone OBJ: GET /api/tileView/fresnelSliceObj/<analysisId>/<tileId>
 *
 * Tile obstruction_ids from GET /api/tileview/terrain/tileOverview/<tileId>
 * are keyed by type: { building: [id, ...], permit: [id, ...], ... }
 *
 * OBJ coord system: X=local easting, Y=local northing, Z=elevation (ft)
 * Three.js transform: rotation.x=-PI/2, position=(-250, 0, 250)
 *
 * TIFF layout (from numpy shape (easting=500, northing=500)):
 *   TIFF row = easting axis, TIFF col = northing axis
 *   rasters[0][easting * width + northing] = height in inches
 */
import { useRef, useMemo, useEffect, useState, useCallback } from 'react'
import { Canvas, useThree } from '@react-three/fiber'
import { OrbitControls } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import type { ThreeEvent } from '@react-three/fiber'

interface TileOverviewData {
  obstruction_ids: Record<string, string[]>  // { type: [id, ...] }
}

interface HeightmapInfo {
  data: Uint16Array | Int16Array | Float32Array
  width: number   // = northing count (TIFF cols)
  height: number  // = easting count (TIFF rows)
}

interface TerrainInfo {
  minFt: number
  maxFt: number
  heightRange: number
}

interface TileData {
  tileOverview: TileOverviewData
  heightmap: HeightmapInfo
  zoneAvailable: boolean
}

// Flat list of obstruction {type, id} pairs
interface ObsEntry {
  type: string
  id: string
}

const OBJ_ROTATION: [number, number, number] = [-Math.PI / 2, 0, 0]
const OBJ_POSITION: [number, number, number] = [-250, 0, 250]
const OBS_COLORS = [0xff6633, 0xffaa00, 0xff3388, 0xaa44ff, 0x44ccff]

// ── Data fetching hook ────────────────────────────────────────────────────────
function useTileData(tileId: string | null, analysisId: string | null): TileData | null | 'loading' | 'error' {
  const [state, setState] = useState<TileData | null | 'loading' | 'error'>(null)

  useEffect(() => {
    if (!tileId || !analysisId) { setState(null); return }
    setState('loading')
    let cancelled = false

    const run = async () => {
      try {
        // 1. Fetch tile overview (obstruction IDs by type)
        const overviewRes = await fetch(`/api/tileview/terrain/tileOverview/${tileId}`)
        if (!overviewRes.ok) throw new Error(`Tile overview HTTP ${overviewRes.status}`)
        const tileOverview = await overviewRes.json() as TileOverviewData
        if (cancelled) return

        // 2. Check if Fresnel zone OBJ is available (204 = not in this tile)
        const zoneRes = await fetch(
          `/api/tileView/fresnelSliceObj/${analysisId}/${tileId}`,
          { method: 'HEAD' }
        )
        const zoneAvailable = zoneRes.ok && zoneRes.status !== 204
        if (cancelled) return

        // 3. Fetch + decode TIFF heightmap
        const tiffRes = await fetch(`/api/tileview/terrain/heightRaster/${tileId}`)
        if (!tiffRes.ok) throw new Error(`Heightmap HTTP ${tiffRes.status}`)
        const buf = await tiffRes.arrayBuffer()
        if (cancelled) return

        const { fromArrayBuffer } = await import('geotiff')
        const tiff = await fromArrayBuffer(buf)
        const image = await tiff.getImage()
        const rasters = await image.readRasters()
        if (cancelled) return

        const heightmap: HeightmapInfo = {
          data: rasters[0] as Uint16Array,
          width: image.getWidth(),
          height: image.getHeight(),
        }

        setState({ tileOverview, heightmap, zoneAvailable })
      } catch {
        if (!cancelled) setState('error')
      }
    }

    run()
    return () => { cancelled = true }
  }, [tileId, analysisId])

  return state
}

// ── Terrain mesh (TIFF displacement) ─────────────────────────────────────────
interface TerrainMeshProps {
  heightmap: HeightmapInfo
  orthoUrl: string
  showOrtho: boolean
  onReady: (info: TerrainInfo) => void
  onLoaded: () => void
  onOrthoLoaded: () => void
}

function TerrainMesh({ heightmap, orthoUrl, showOrtho, onReady, onLoaded, onOrthoLoaded }: TerrainMeshProps) {
  const { data, width, height } = heightmap

  const [geometry, setGeometry] = useState<THREE.BufferGeometry | null>(null)
  const [texture,  setTexture]  = useState<THREE.Texture | null>(null)

  const onReadyRef = useRef(onReady)
  onReadyRef.current = onReady
  const onLoadedRef = useRef(onLoaded)
  onLoadedRef.current = onLoaded
  const onOrthoLoadedRef = useRef(onOrthoLoaded)
  onOrthoLoadedRef.current = onOrthoLoaded

  useEffect(() => {
    // width = northing count (TIFF cols), height = easting count (TIFF rows)
    const geo = new THREE.PlaneGeometry(500, 500, width - 1, height - 1)
    geo.rotateX(-Math.PI / 2)
    const pos = geo.attributes['position'] as THREE.BufferAttribute

    let minFt = Infinity, maxFt = -Infinity
    for (let i = 0; i < pos.count; i++) {
      const col = i % width          // easting index
      const row = Math.floor(i / width)  // PlaneGeometry row (0 = north)
      const northing = (height - 1) - row  // northing index (row 0 → max northing)
      const heightIn = data[col * width + northing]  // rasters[0][easting * width + northing]
      const heightFt = heightIn / 12.0
      pos.setY(i, heightFt)
      if (heightIn > 0) {
        minFt = Math.min(minFt, heightFt)
        maxFt = Math.max(maxFt, heightFt)
      }
    }
    if (minFt === Infinity) { minFt = 0; maxFt = 0 }
    pos.needsUpdate = true
    geo.computeVertexNormals()
    setGeometry(geo)
    onReadyRef.current({ minFt, maxFt, heightRange: Math.max(maxFt - minFt, 1) })
    onLoadedRef.current()
  }, [data, width, height])

  useEffect(() => {
    if (!orthoUrl) return
    const loader = new THREE.TextureLoader()
    loader.load(orthoUrl, tex => { setTexture(tex); onOrthoLoadedRef.current() }, undefined, () => {})
    return () => {}
  }, [orthoUrl])

  if (!geometry) return null

  const mat = (texture && showOrtho)
    ? new THREE.MeshStandardMaterial({ map: texture, roughness: 0.85, metalness: 0 })
    : new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.85, metalness: 0 })

  return <mesh geometry={geometry} material={mat} />
}

// ── Imperative OBJ loader (avoids useLoader's global Suspense cache) ──────────
function useObjLoader(url: string): THREE.Group | null {
  const [obj, setObj] = useState<THREE.Group | null>(null)
  useEffect(() => {
    let cancelled = false
    setObj(null)
    new OBJLoader().load(url, loaded => { if (!cancelled) setObj(loaded) })
    return () => { cancelled = true }
  }, [url])
  return obj
}

// ── Zone OBJ ──────────────────────────────────────────────────────────────────
function ZoneObj({ analysisId, tileId, onLoaded, visible }: { analysisId: string; tileId: string; onLoaded: () => void; visible: boolean }) {
  const obj = useObjLoader(`/api/tileView/fresnelSliceObj/${analysisId}/${tileId}`)
  const onLoadedRef = useRef(onLoaded)
  onLoadedRef.current = onLoaded
  useEffect(() => {
    if (!obj) return
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        (child as THREE.Mesh).material = new THREE.MeshStandardMaterial({
          color: 0xcc44ff, transparent: true, opacity: 0.5,
          depthWrite: false, side: THREE.DoubleSide, roughness: 0.4,
        })
      }
    })
    onLoadedRef.current()
  }, [obj])
  if (!obj) return null
  return <primitive object={obj} rotation={OBJ_ROTATION} position={OBJ_POSITION} visible={visible} />
}

// ── Obstruction OBJ ───────────────────────────────────────────────────────────
function ObsObj({ type, obsId, tileId, color, onHit, onLoaded, onBoundsReady, visible }: {
  type: string; obsId: string; tileId: string; color: number
  onHit: (key: string) => void; onLoaded: () => void
  onBoundsReady: (key: string, box: THREE.Box3) => void
  visible: boolean
}) {
  const obj = useObjLoader(`/api/tileview/terrain/obstructionObj/${type}/${obsId}/${tileId}`)
  const key = `${type}/${obsId}`
  const onLoadedRef = useRef(onLoaded)
  const onBoundsRef = useRef(onBoundsReady)
  onLoadedRef.current = onLoaded
  onBoundsRef.current = onBoundsReady
  useEffect(() => {
    if (!obj) return
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        (child as THREE.Mesh).material = new THREE.MeshStandardMaterial({
          color, roughness: 0.65, metalness: 0.05,
        })
      }
    })
    // By the time this effect runs R3F has already applied rotation+position to obj,
    // so setFromObject gives the correct world-space box directly.
    onBoundsRef.current(key, new THREE.Box3().setFromObject(obj))
    onLoadedRef.current()
  }, [obj, color, key])
  if (!obj) return null
  return (
    <primitive
      object={obj}
      rotation={OBJ_ROTATION}
      position={OBJ_POSITION}
      visible={visible}
      onClick={(e: ThreeEvent<MouseEvent>) => { e.stopPropagation(); onHit(key) }}
    />
  )
}

// ── Camera setup ──────────────────────────────────────────────────────────────
function CameraSetup({ heightRange, midFt, resetSeq, controlsRef }: {
  heightRange: number; midFt: number; resetSeq: number; controlsRef: React.RefObject<any>
}) {
  const { camera } = useThree()

  useEffect(() => {
    const camDist = Math.max(heightRange * 3, 300)
    const oc = controlsRef.current
    if (oc) {
      oc.target.set(0, midFt, 0)
      oc.update()
    }
    camera.position.set(camDist * 0.6, midFt + camDist * 0.8, camDist * 0.9)
    camera.lookAt(0, midFt, 0)
  }, [heightRange, midFt, camera, controlsRef, resetSeq])

  return null
}

// ── Frame obstruction ─────────────────────────────────────────────────────────
function FrameTarget({ box, controlsRef }: { box: THREE.Box3; controlsRef: React.RefObject<any> }) {
  const { camera } = useThree()
  useEffect(() => {
    const oc = controlsRef.current
    if (!oc) return
    const center = new THREE.Vector3()
    box.getCenter(center)
    const size = new THREE.Vector3()
    box.getSize(size)
    const maxDim = Math.max(size.x, size.y, size.z)
    const fovRad = 50 * (Math.PI / 180)
    const distance = (maxDim / (2 * Math.tan(fovRad / 2))) * 2.5
    // Preserve current viewing angle, just reposition
    const dir = camera.position.clone().sub(oc.target).normalize()
    oc.target.copy(center)
    camera.position.copy(center.clone().addScaledVector(dir, distance))
    oc.update()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [box])
  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────
interface SceneProps {
  tileId: string
  analysisId: string
  tileData: TileData
  orthoUrl: string
  obstructions: ObsEntry[]
  visibility: Record<string, boolean>
  frameKey: string | null
  onObsClick: (key: string) => void
  onItemLoaded: (key: string) => void
}

function Scene({ tileId, analysisId, tileData, orthoUrl, obstructions, visibility, frameKey, onObsClick, onItemLoaded }: SceneProps) {
  const [terrainInfo, setTerrainInfo] = useState<TerrainInfo | null>(null)
  const heightRange = terrainInfo?.heightRange ?? 1
  const midFt = terrainInfo ? (terrainInfo.minFt + terrainInfo.maxFt) / 2 : 0

  const controlsRef = useRef<any>(null)
  // Map of loaded bounding boxes, keyed by type/id
  const boundsRef = useRef<Record<string, THREE.Box3>>({})
  // Key pending a frame (selected before its OBJ finished loading)
  const pendingFrameRef = useRef<string | null>(null)
  const [frameBox, setFrameBox] = useState<THREE.Box3 | null>(null)
  const [cameraResetSeq, setCameraResetSeq] = useState(0)
  const prevFrameKeyRef = useRef<string | null>(null)

  useEffect(() => {
    const prev = prevFrameKeyRef.current
    prevFrameKeyRef.current = frameKey
    if (!frameKey) {
      if (prev !== null) setCameraResetSeq(s => s + 1)
      return
    }
    const box = boundsRef.current[frameKey]
    if (box) {
      setFrameBox(box.clone())
    } else {
      pendingFrameRef.current = frameKey
    }
  }, [frameKey])

  const handleBoundsReady = useCallback((key: string, box: THREE.Box3) => {
    boundsRef.current[key] = box
    if (pendingFrameRef.current === key) {
      pendingFrameRef.current = null
      setFrameBox(box.clone())
    }
  }, [])

  return (
    <>
      <ambientLight intensity={1.2} />
      <directionalLight
        position={[-220, Math.max(heightRange * 1.5 + 80, 100), 180]}
        intensity={1.5}
        color={0xfff8e8}
      />
      <directionalLight
        position={[220, Math.max(heightRange * 0.5, 50), -180]}
        intensity={0.4}
        color={0x6080b0}
      />

      {visibility['terrain'] !== false && (
        <TerrainMesh
          heightmap={tileData.heightmap}
          orthoUrl={orthoUrl}
          showOrtho={visibility['ortho'] !== false}
          onReady={setTerrainInfo}
          onLoaded={() => onItemLoaded('terrain')}
          onOrthoLoaded={() => onItemLoaded('ortho')}
        />
      )}

      {tileData.zoneAvailable && (
        <ZoneObj analysisId={analysisId} tileId={tileId} onLoaded={() => onItemLoaded('zone')} visible={visibility['zone'] !== false} />
      )}
      {obstructions.map(({ type, id }, i) => (
        <ObsObj
          key={`${type}/${id}`}
          type={type}
          obsId={id}
          tileId={tileId}
          color={OBS_COLORS[i % OBS_COLORS.length]}
          onHit={onObsClick}
          onLoaded={() => onItemLoaded(`${type}/${id}`)}
          onBoundsReady={handleBoundsReady}
          visible={visibility[`${type}/${id}`] !== false}
        />
      ))}

      <OrbitControls
        ref={controlsRef}
        enableDamping
        dampingFactor={0.08}
        minDistance={10}
        maxDistance={5000}
        maxPolarAngle={Math.PI * 0.9}
      />
      {terrainInfo && <CameraSetup heightRange={heightRange} midFt={midFt} resetSeq={cameraResetSeq} controlsRef={controlsRef} />}
      {frameBox && <FrameTarget box={frameBox} controlsRef={controlsRef} />}
    </>
  )
}

// ── Obstruction overview fetching ────────────────────────────────────────────
interface ObsOverview {
  obstruction_type: string
  attributes: Record<string, string | number | null>
}

function useObstructionOverviews(obstructions: ObsEntry[]): Record<string, ObsOverview | null> {
  const [overviews, setOverviews] = useState<Record<string, ObsOverview | null>>({})
  const key = obstructions.map(o => `${o.type}/${o.id}`).join(',')
  useEffect(() => {
    if (!obstructions.length) { setOverviews({}); return }
    let cancelled = false
    Promise.all(
      obstructions.map(({ type, id }) =>
        fetch(`/api/tileview/terrain/obstructionOverview/${type}/${id}`)
          .then(r => r.ok ? r.json() : null)
          .catch(() => null)
          .then(data => ({ key: `${type}/${id}`, data: data as ObsOverview | null }))
      )
    ).then(results => {
      if (cancelled) return
      const map: Record<string, ObsOverview | null> = {}
      for (const { key: k, data } of results) map[k] = data
      setOverviews(map)
    })
    return () => { cancelled = true }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return overviews
}

function obsEntryLabel(overview: ObsOverview | null | undefined, type: string): string {
  const typePart = type.replace(/_/g, ' ')
  const addr = overview?.attributes?.street_addr as string | undefined
  const bin = overview?.attributes?.bin as string | undefined
  const name = addr ?? (bin ? `BIN ${bin}` : null)
  return name ? `${name} (${typePart})` : `(${typePart})`
}

// ── Obstruction detail panel ──────────────────────────────────────────────────
const ATTR_LABELS: Record<string, string> = {
  street_addr: 'Address',
  borough: 'Borough',
  bin: 'BIN',
  bbl: 'BBL',
  ground_elevation: 'Ground elev.',
  height_roof: 'Roof height',
  job_id: 'Job ID',
  job_application_system: 'App. system',
  job_filing_date: 'Filing date',
  job_approval_date: 'Approval date',
  tco_date: 'TCO date',
  construction_year: 'Const. year',
  last_status_type: 'Status',
  geom_source: 'Geom. source',
}
const ATTR_UNITS: Record<string, string> = { ground_elevation: ' ft', height_roof: ' ft' }

const DOB_SEARCH_URLS: Record<string, string> = {
  DOB_BIS: 'https://a810-bisweb.nyc.gov/bisweb/bsqpm01.jsp',
  DOB_NOW: 'https://a810-dobnow.nyc.gov/publish/Index.html#!/search',
}

function ObsDetailPanel({ obsKey, overview, onClose }: {
  obsKey: string
  overview: ObsOverview | null | undefined
  onClose: () => void
}) {
  const type = obsKey.split('/')[0]
  const title = obsEntryLabel(overview ?? null, type)
  const attrs = overview?.attributes ?? {}
  const jobId = attrs.job_id as string | undefined
  const appSystem = attrs.job_application_system as string | undefined
  const dobUrl = appSystem ? DOB_SEARCH_URLS[appSystem] : undefined

  const handleCopy = () => {
    if (!jobId) return
    navigator.clipboard.writeText(String(jobId))
  }

  return (
    <div style={styles.detailPanel}>
      <div style={styles.detailHeader}>
        <span style={styles.detailTitle}>{title}</span>
        <button style={styles.obsClose} onClick={onClose}>×</button>
      </div>
      <div style={styles.detailBody}>
        {Object.entries(attrs).map(([k, v]) => {
          if (v === null || v === undefined) return null
          const label = ATTR_LABELS[k] ?? k.replace(/_/g, ' ')
          const unit = ATTR_UNITS[k] ?? ''

          if (k === 'job_id') {
            return (
              <div key={k} style={styles.detailRow}>
                <span style={styles.detailKey}>{label}</span>
                <span style={{ ...styles.detailVal, display: 'flex', alignItems: 'center', gap: 6 }}>
                  <span>{String(v)}</span>
                  <button
                    style={styles.copyBtn}
                    onClick={handleCopy}
                    title="Copy job number"
                  >⎘</button>
                  {dobUrl && (
                    <a
                      href={dobUrl}
                      target="_blank"
                      rel="noreferrer"
                      style={styles.dobLink}
                      title={`Search ${appSystem}`}
                    >↗</a>
                  )}
                </span>
              </div>
            )
          }

          return (
            <div key={k} style={styles.detailRow}>
              <span style={styles.detailKey}>{label}</span>
              <span style={styles.detailVal}>{String(v)}{unit}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}

// ── Legend ────────────────────────────────────────────────────────────────────
function LegendSwatch({ color, loaded }: { color: string; loaded: boolean }) {
  if (!loaded) {
    return (
      <span style={{
        display: 'inline-block', width: 11, height: 11, borderRadius: '50%', flexShrink: 0,
        border: '1.5px solid rgba(255,255,255,0.12)', borderTopColor: '#8b949e',
        animation: 'tile3d-spin 0.7s linear infinite',
      }} />
    )
  }
  return <span style={{ ...styles.swatch, background: color }} />
}

function Legend({
  tileData, obstructions, obsOverviews, visibility, loadedKeys, activeKey, onToggle, onSelect,
}: {
  tileData: TileData
  obstructions: ObsEntry[]
  obsOverviews: Record<string, ObsOverview | null>
  visibility: Record<string, boolean>
  loadedKeys: Record<string, true>
  activeKey: string | null
  onToggle: (key: string) => void
  onSelect: (key: string) => void
}) {
  const staticItems = [
    { key: 'terrain', color: '#ffffff', label: 'Terrain Geometry', obsKey: null as string | null },
    { key: 'ortho', color: '#4a90d9', label: 'Terrain Textures', obsKey: null as string | null },
    ...(tileData.zoneAvailable ? [{ key: 'zone', color: '#cc44ff', label: 'Fresnel Zone', obsKey: null as string | null }] : []),
  ]
  const obsItems = obstructions.map(({ type, id }, i) => ({
    key: `${type}/${id}`,
    color: '#' + OBS_COLORS[i % OBS_COLORS.length].toString(16).padStart(6, '0'),
    label: obsEntryLabel(obsOverviews[`${type}/${id}`], type),
    obsKey: `${type}/${id}`,
  }))
  const items = [...staticItems, ...obsItems]

  return (
    <div style={styles.legend}>
      <style>{`@keyframes tile3d-spin { to { transform: rotate(360deg); } }`}</style>
      <div style={styles.legendTitle}>Scene objects</div>
      {items.map(item => (
        <div key={item.key} style={styles.legendItem}>
          <input
            type="checkbox"
            checked={visibility[item.key] !== false}
            onChange={() => onToggle(item.key)}
            style={styles.checkbox}
          />
          <LegendSwatch color={item.color} loaded={!!loadedKeys[item.key]} />
          <span
            style={{
              ...styles.legendLabel,
              ...(item.obsKey ? {
                cursor: 'pointer',
                color: activeKey === item.obsKey ? '#e6edf3' : '#8b949e',
              } : {}),
            }}
            onClick={item.obsKey ? () => onSelect(item.obsKey!) : undefined}
          >
            {item.label}
          </span>
        </div>
      ))}
    </div>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────
interface Tile3DViewerProps {
  tileId: string | null
  analysisId: string | null
}

export default function Tile3DViewer({ tileId, analysisId }: Tile3DViewerProps) {
  const tileData = useTileData(tileId, analysisId)
  const [activeObs, setActiveObs] = useState<string | null>(null)
  const [visibility, setVisibility] = useState<Record<string, boolean>>({})
  const [loadedKeys, setLoadedKeys] = useState<Record<string, true>>({})

  // Reset visibility and loaded state when tile changes
  useEffect(() => { setVisibility({}); setLoadedKeys({}) }, [tileId, analysisId])

  const handleItemLoaded = useCallback((key: string) =>
    setLoadedKeys(prev => prev[key] ? prev : { ...prev, [key]: true })
  , [])

  const toggleVisibility = (key: string) =>
    setVisibility(v => ({ ...v, [key]: v[key] === false ? true : false }))

  // Must be before early returns to satisfy Rules of Hooks
  const obstructions = useMemo<ObsEntry[]>(
    () => tileData && tileData !== 'loading' && tileData !== 'error'
      ? Object.entries(tileData.tileOverview.obstruction_ids)
          .flatMap(([type, ids]) => ids.map(id => ({ type, id })))
      : [],
    [tileData],
  )
  const obsOverviews = useObstructionOverviews(obstructions)

  if (!tileId || !analysisId) {
    return <div style={styles.placeholder}>Click a tile on the map to open the 3D view</div>
  }

  if (tileData === 'error') {
    return <div style={styles.placeholder}><span style={{ color: '#ff4444' }}>Failed to load tile data</span></div>
  }

  if (tileData === 'loading' || tileData === null) {
    return (
      <div style={styles.placeholder}>
        <span style={{ color: '#484f58', fontFamily: 'monospace', fontSize: 12 }}>
          Loading tile data…
        </span>
      </div>
    )
  }

  const orthoUrl = `/api/tileview/terrain/orthoImage/${tileId}`

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
      <div style={{ flex: 1 }}>
        <Canvas
          camera={{ fov: 50, near: 0.5, far: 200000 }}
          gl={{ antialias: true }}
          style={{ background: '#0e1117' }}
        >
          <Scene
            tileId={tileId}
            analysisId={analysisId}
            tileData={tileData}
            orthoUrl={orthoUrl}
            obstructions={obstructions}
            visibility={visibility}
            frameKey={activeObs}
            onObsClick={setActiveObs}
            onItemLoaded={handleItemLoaded}
          />
        </Canvas>
      </div>

      {activeObs && (
        <ObsDetailPanel
          obsKey={activeObs}
          overview={obsOverviews[activeObs]}
          onClose={() => setActiveObs(null)}
        />
      )}

      <Legend
        tileData={tileData}
        obstructions={obstructions}
        obsOverviews={obsOverviews}
        visibility={visibility}
        loadedKeys={loadedKeys}
        activeKey={activeObs}
        onToggle={toggleVisibility}
        onSelect={key => setActiveObs(prev => prev === key ? null : key)}
      />
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  placeholder: {
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    textAlign: 'center',
    padding: 20,
    color: '#3d444d',
    fontFamily: 'monospace',
    fontSize: 13,
  },
  legend: {
    flexShrink: 0,
    maxHeight: '35%',
    overflowY: 'auto',
    background: '#0a0d13',
    borderTop: '1px solid #1c2128',
    padding: '8px 12px',
  },
  legendTitle: {
    color: '#3d444d',
    fontFamily: 'monospace',
    fontSize: 10,
    textTransform: 'uppercase',
    letterSpacing: '0.08em',
    marginBottom: 6,
  },
  legendItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '3px 0',
    cursor: 'pointer',
    userSelect: 'none',
  },
  checkbox: {
    accentColor: '#388bfd',
    cursor: 'pointer',
    flexShrink: 0,
  },
  swatch: {
    width: 11,
    height: 11,
    borderRadius: 2,
    flexShrink: 0,
    border: '1px solid rgba(255,255,255,0.12)',
  },
  legendLabel: {
    fontSize: 11,
    fontFamily: 'monospace',
    color: '#8b949e',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  detailPanel: {
    position: 'absolute',
    top: 10,
    right: 10,
    width: 240,
    background: 'rgba(13, 17, 23, 0.94)',
    border: '1px solid #1c2128',
    borderRadius: 6,
    fontFamily: 'monospace',
    fontSize: 11,
    pointerEvents: 'auto',
    zIndex: 10,
  },
  detailHeader: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    padding: '8px 10px 6px',
    borderBottom: '1px solid #1c2128',
    gap: 8,
  },
  detailTitle: {
    color: '#e6edf3',
    fontSize: 11,
    lineHeight: 1.4,
  },
  detailBody: {
    padding: '6px 10px 8px',
    display: 'flex',
    flexDirection: 'column' as const,
    gap: 3,
  },
  detailRow: {
    display: 'flex',
    justifyContent: 'space-between',
    gap: 12,
  },
  detailKey: {
    color: '#6e7681',
    flexShrink: 0,
  },
  detailVal: {
    color: '#adbac7',
    textAlign: 'right' as const,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  obsClose: {
    color: '#666',
    fontSize: 16,
    lineHeight: 1,
    cursor: 'pointer',
    background: 'none',
    border: 'none',
    flexShrink: 0,
    padding: 0,
  },
  copyBtn: {
    color: '#000000',
    fontSize: 18,
    cursor: 'pointer',
    background: '#ffffff',
    border: 'none',
    borderRadius: '50%',
    width: 16,
    height: 16,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
    padding: 0,
    lineHeight: 1,
  },
  dobLink: {
    color: '#ffffff',
    textDecoration: 'none',
    fontSize: 18,
    background: '#388bfd',
    borderRadius: '50%',
    width: 16,
    height: 16,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
    lineHeight: 1,
    paddingBottom: 3,
  },
}
