/**
 * 3D tile viewer using React Three Fiber.
 *
 * - Displacement-mapped PlaneGeometry terrain (from heightmap PNG data URI)
 * - Optional ortho texture on terrain
 * - Fresnel zone OBJ (translucent, from /api/tile-3d/<jobId>/zone.obj)
 * - Obstruction OBJs (from /api/tile-3d/<jobId>/<obsId>.obj)
 * - OrbitControls
 * - Scrollable legend with visibility toggles
 *
 * OBJ coordinate system: X=local easting, Y=local northing, Z=elevation (ft)
 * Three.js world: rotation.x=-PI/2, position=(-250, 0, 250) centers the tile
 */
import { useRef, useMemo, useEffect, useState, Suspense } from 'react'
import { Canvas, useLoader, useThree } from '@react-three/fiber'
import { OrbitControls } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { useJob } from '../../hooks/useJob'
import type { ThreeEvent } from '@react-three/fiber'

interface HeightmapData {
  url: string
  min_height_in: number
  max_height_in: number
}

interface ObsInfo {
  type: string
  attributes: Record<string, unknown>
}

export interface Tile3DResult {
  tile_id: string
  job_id: string
  heightmap: HeightmapData | null
  ortho_texture: string | null
  zone_obj_available: boolean
  obstruction_ids: string[]
  obs_info: Record<string, ObsInfo>
}

interface Tile3DViewerProps {
  jobId: string | null
  tileId: string | null
  tileHeightmap?: HeightmapData | null
  tileOrtho?: string | null
}

const OBJ_ROTATION: [number, number, number] = [-Math.PI / 2, 0, 0]
const OBJ_POSITION: [number, number, number] = [-250, 0, 250]
const OBS_COLORS = [0xff6633, 0xffaa00, 0xff3388, 0xaa44ff, 0x44ccff]

// ── Terrain mesh ──────────────────────────────────────────────────────────────
interface TerrainInfo {
  minFt: number
  maxFt: number
  heightRange: number
}

interface TerrainMeshProps {
  heightmapData: HeightmapData
  orthoUrl: string | null | undefined
  onReady: (info: TerrainInfo) => void
}

function TerrainMesh({ heightmapData, orthoUrl, onReady }: TerrainMeshProps) {
  const { min_height_in, max_height_in, url } = heightmapData
  const minFt = min_height_in / 12
  const maxFt = max_height_in / 12
  const heightRange = Math.max(maxFt - minFt, 1)

  const [geometry, setGeometry] = useState<THREE.BufferGeometry | null>(null)
  const [texture,  setTexture]  = useState<THREE.Texture | null>(null)

  // Decode heightmap PNG via offscreen canvas → displace vertices
  useEffect(() => {
    const img = new Image()
    img.onload = () => {
      const cvs = document.createElement('canvas')
      cvs.width = 500; cvs.height = 500
      const ctx = cvs.getContext('2d')!
      ctx.drawImage(img, 0, 0, 500, 500)
      const pixels = ctx.getImageData(0, 0, 500, 500).data  // RGBA, row 0 = north

      const geo = new THREE.PlaneGeometry(500, 500, 499, 499)
      geo.rotateX(-Math.PI / 2)
      const pos = geo.attributes['position'] as THREE.BufferAttribute
      for (let i = 0; i < pos.count; i++) {
        const col = i % 500
        const row = Math.floor(i / 500)  // row 0 = north
        const t = pixels[(row * 500 + col) * 4] / 255
        pos.setY(i, minFt + t * heightRange)
      }
      pos.needsUpdate = true
      geo.computeVertexNormals()
      setGeometry(geo)
      if (onReady) onReady({ minFt, maxFt, heightRange })
    }
    img.src = url
  }, [url, minFt, maxFt, heightRange, onReady])

  // Load ortho texture
  useEffect(() => {
    if (!orthoUrl) return
    const loader = new THREE.TextureLoader()
    loader.load(orthoUrl, tex => setTexture(tex), undefined, () => {})
    return () => {}
  }, [orthoUrl])

  if (!geometry) return null

  const mat = texture
    ? new THREE.MeshBasicMaterial({ map: texture })
    : new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.85, metalness: 0 })

  return <mesh geometry={geometry} material={mat} />
}

// ── Zone OBJ ──────────────────────────────────────────────────────────────────
interface ZoneObjProps {
  jobId: string
}

function ZoneObj({ jobId }: ZoneObjProps) {
  const url = `/api/tile-3d/${jobId}/zone.obj`
  const obj = useLoader(OBJLoader, url)

  useEffect(() => {
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        (child as THREE.Mesh).material = new THREE.MeshStandardMaterial({
          color: 0xcc44ff, transparent: true, opacity: 0.5,
          depthWrite: false, side: THREE.DoubleSide, roughness: 0.4,
        })
      }
    })
  }, [obj])

  return (
    <primitive
      object={obj}
      rotation={OBJ_ROTATION}
      position={OBJ_POSITION}
    />
  )
}

// ── Obstruction OBJ ───────────────────────────────────────────────────────────
interface ObsObjProps {
  jobId: string
  obsId: string
  color: number
  onHit: (obsId: string) => void
}

function ObsObj({ jobId, obsId, color, onHit }: ObsObjProps) {
  const url = `/api/tile-3d/${jobId}/${obsId}.obj`
  const obj = useLoader(OBJLoader, url)

  useEffect(() => {
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        (child as THREE.Mesh).material = new THREE.MeshStandardMaterial({
          color, roughness: 0.65, metalness: 0.05,
        })
      }
    })
  }, [obj, color])

  return (
    <primitive
      object={obj}
      rotation={OBJ_ROTATION}
      position={OBJ_POSITION}
      onClick={(e: ThreeEvent<MouseEvent>) => { e.stopPropagation(); onHit(obsId) }}
    />
  )
}

// ── Camera setup ──────────────────────────────────────────────────────────────
interface CameraSetupProps {
  heightRange: number
  midFt: number
}

function CameraSetup({ heightRange, midFt }: CameraSetupProps) {
  const { camera, controls } = useThree()

  useEffect(() => {
    const camDist = Math.max(heightRange * 3, 300)
    if (controls) {
      const orbitControls = controls as unknown as {
        target: THREE.Vector3
        update: () => void
      }
      orbitControls.target.set(0, midFt, 0)
      orbitControls.update()
    }
    camera.position.set(camDist * 0.6, midFt + camDist * 0.8, camDist * 0.9)
    camera.lookAt(0, midFt, 0)
  }, [heightRange, midFt, camera, controls])

  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────
interface SceneProps {
  result: Tile3DResult
  onObsClick: (obsId: string) => void
}

function Scene({ result, onObsClick }: SceneProps) {
  const [terrainInfo, setTerrainInfo] = useState<TerrainInfo | null>(null)

  const heightRange = terrainInfo?.heightRange ?? 1
  const midFt = terrainInfo ? (terrainInfo.minFt + terrainInfo.maxFt) / 2 : 0

  return (
    <>
      <ambientLight intensity={0.5} color={0x334466} />
      <directionalLight
        position={[-220, Math.max(heightRange * 1.5 + 80, 100), 180]}
        intensity={4.5}
        color={0xfff8e8}
      />
      <directionalLight
        position={[220, Math.max(heightRange * 0.5, 50), -180]}
        intensity={0.4}
        color={0x6080b0}
      />

      {result.heightmap && (
        <TerrainMesh
          heightmapData={result.heightmap}
          orthoUrl={result.ortho_texture}
          onReady={setTerrainInfo}
        />
      )}

      {result.zone_obj_available && (
        <Suspense fallback={null}>
          <ZoneObj jobId={result.job_id} />
        </Suspense>
      )}

      {result.obstruction_ids?.map((obsId, i) => (
        <Suspense key={obsId} fallback={null}>
          <ObsObj
            jobId={result.job_id}
            obsId={obsId}
            color={OBS_COLORS[i % OBS_COLORS.length]}
            onHit={onObsClick}
          />
        </Suspense>
      ))}

      <OrbitControls
        enableDamping
        dampingFactor={0.08}
        minDistance={10}
        maxDistance={5000}
        maxPolarAngle={Math.PI * 0.9}
      />
      {terrainInfo && (
        <CameraSetup heightRange={heightRange} midFt={midFt} />
      )}
    </>
  )
}

// ── Legend ────────────────────────────────────────────────────────────────────
interface LegendItem {
  key: string
  color: string
  label: string
}

interface LegendProps {
  result: Tile3DResult
}

function Legend({ result }: LegendProps) {
  const items: LegendItem[] = [
    { key: 'terrain', color: '#ffffff', label: 'Terrain' },
    ...(result.zone_obj_available ? [{ key: 'zone', color: '#cc44ff', label: 'Fresnel Zone' }] : []),
    ...(result.obstruction_ids?.map((id, i) => ({
      key: id,
      color: '#' + OBS_COLORS[i % OBS_COLORS.length].toString(16).padStart(6, '0'),
      label: `${(result.obs_info?.[id]?.type || 'obstruction').replace(/_/g, ' ')} · ${id.slice(0, 8)}`,
    })) ?? []),
  ]

  if (items.length === 0) return null

  return (
    <div style={styles.legend}>
      <div style={styles.legendTitle}>Scene objects</div>
      {items.map(item => (
        <div key={item.key} style={styles.legendItem}>
          <span style={{ ...styles.swatch, background: item.color }} />
          <span style={styles.legendLabel}>{item.label}</span>
        </div>
      ))}
    </div>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────
export default function Tile3DViewer({ jobId, tileId: _tileId, tileHeightmap: _tileHeightmap, tileOrtho: _tileOrtho }: Tile3DViewerProps) {
  const job = useJob(jobId)
  const [activeObs, setActiveObs] = useState<string | null>(null)

  const result = job?.status === 'done' ? (job.result as unknown as Tile3DResult) : null

  if (!jobId) {
    return (
      <div style={styles.placeholder}>
        Click a tile on the map to open the 3D view
      </div>
    )
  }

  if (job?.status === 'error') {
    return (
      <div style={styles.placeholder}>
        <span style={{ color: '#ff4444' }}>
          Error: {job.error?.trim().split('\n').pop()}
        </span>
      </div>
    )
  }

  if (!result) {
    return (
      <div style={styles.placeholder}>
        <span style={{ color: '#484f58', fontFamily: 'monospace', fontSize: 12 }}>
          {job?.message || 'Loading…'}
        </span>
      </div>
    )
  }

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
      <div style={{ flex: 1 }}>
        <Canvas
          camera={{ fov: 50, near: 0.5, far: 200000 }}
          gl={{ antialias: true }}
          style={{ background: '#0e1117' }}
        >
          <Scene result={result} onObsClick={setActiveObs} />
        </Canvas>
      </div>

      {/* Obstruction label */}
      {activeObs && (
        <div style={styles.obsLabel}>
          <span>{(result.obs_info?.[activeObs]?.type || 'obstruction').replace(/_/g, ' ')}</span>
          <span style={{ color: '#484f58', marginLeft: 8, fontSize: 11 }}>{activeObs}</span>
          <button style={styles.obsClose} onClick={() => setActiveObs(null)}>×</button>
        </div>
      )}

      {/* Legend */}
      <Legend result={result} />
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
  obsLabel: {
    position: 'absolute',
    bottom: 10,
    left: '50%',
    transform: 'translateX(-50%)',
    background: 'rgba(0,0,0,0.75)',
    color: '#fff',
    fontFamily: 'monospace',
    fontSize: 11,
    padding: '4px 12px',
    borderRadius: 4,
    display: 'flex',
    alignItems: 'center',
    gap: 4,
    whiteSpace: 'nowrap',
    pointerEvents: 'auto',
  },
  obsClose: {
    color: '#666',
    fontSize: 16,
    marginLeft: 6,
    cursor: 'pointer',
    background: 'none',
    border: 'none',
  },
}
