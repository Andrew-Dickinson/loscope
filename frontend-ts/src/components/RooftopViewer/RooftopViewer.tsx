/**
 * Interactive 3D rooftop viewer using React Three Fiber.
 *
 * - Loads building terrain OBJ from /api/rooftop/<jobId>/terrain.obj
 * - Colors terrain with Voronoi shader (nearest sample point status)
 * - Renders measurement point spheres as InstancedMesh
 * - Click a sphere → onPointClick(point) with absolute NYS coords
 */
import { useRef, useMemo, useEffect, Suspense } from 'react'
import { Canvas, useLoader, useThree } from '@react-three/fiber'
import { OrbitControls } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { buildVoronoiMaterial } from './VoronoiMaterial'
import type { ThreeEvent } from '@react-three/fiber'

export interface SamplePoint {
  x: number
  y: number
  z: number
  s: string
  nys_e: number
  nys_n: number
  nys_z: number
}

export interface RooftopSummary {
  n_clear: number
  n_partial: number
  n_full: number
  total: number
}

export interface RooftopResult {
  bin_id: string
  job_id: string
  x_sw: number
  y_sw: number
  display_points: SamplePoint[]
  points: SamplePoint[]
  summary: RooftopSummary
  _nys_b: [number, number, number]
}

interface RooftopViewerProps {
  jobId: string | null
  result: RooftopResult
  onPointClick: (point: SamplePoint) => void
}

const STATUS_COLOR: Record<string, number> = {
  unobstructed:         0x22cc44,
  partially_obstructed: 0xffcc00,
  fully_obstructed:     0xff4444,
}

// ── Terrain mesh ──────────────────────────────────────────────────────────────
interface TerrainMeshProps {
  objUrl: string
  displayPoints: SamplePoint[]
}

function TerrainMesh({ objUrl, displayPoints }: TerrainMeshProps) {
  const obj = useLoader(OBJLoader, objUrl)
  const mat = useMemo(() => buildVoronoiMaterial(displayPoints), [displayPoints])

  useEffect(() => {
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        const mesh = child as THREE.Mesh
        mesh.material = mat
        mesh.geometry.computeVertexNormals()
      }
    })
  }, [obj, mat])

  return <primitive object={obj} />
}

// ── Sample point spheres ──────────────────────────────────────────────────────
interface SamplePointsProps {
  points: SamplePoint[]
  onPointClick: (point: SamplePoint) => void
}

interface IndexedPoint extends SamplePoint {
  _idx: number
}

function SamplePoints({ points, onPointClick }: SamplePointsProps) {
  const dummy = useMemo(() => new THREE.Object3D(), [])

  const groups = useMemo(() => {
    const g: Record<string, IndexedPoint[]> = {}
    points.forEach((pt, idx) => {
      if (!g[pt.s]) g[pt.s] = []
      g[pt.s].push({ ...pt, _idx: idx })
    })
    return g
  }, [points])

  return (
    <>
      {Object.entries(groups).map(([status, pts]) => (
        <SphereGroup
          key={status}
          pts={pts}
          color={STATUS_COLOR[status] ?? 0xffffff}
          dummy={dummy}
          onPointClick={onPointClick}
        />
      ))}
    </>
  )
}

interface SphereGroupProps {
  pts: IndexedPoint[]
  color: number
  dummy: THREE.Object3D
  onPointClick: (point: SamplePoint) => void
}

function SphereGroup({ pts, color, dummy, onPointClick }: SphereGroupProps) {
  const meshRef = useRef<THREE.InstancedMesh>(null)
  const geo = useMemo(() => new THREE.SphereGeometry(0.8, 8, 6), [])
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ color }), [color])

  useEffect(() => {
    const mesh = meshRef.current
    if (!mesh) return
    pts.forEach((pt, i) => {
      dummy.position.set(pt.x, pt.z, -pt.y)
      dummy.updateMatrix()
      mesh.setMatrixAt(i, dummy.matrix)
    })
    mesh.instanceMatrix.needsUpdate = true
  }, [pts, dummy])

  return (
    <instancedMesh
      ref={meshRef}
      args={[geo, mat, pts.length]}
      onClick={(e: ThreeEvent<MouseEvent>) => {
        e.stopPropagation()
        if (e.instanceId !== undefined) onPointClick(pts[e.instanceId])
      }}
    />
  )
}

// ── Camera auto-fit ───────────────────────────────────────────────────────────
interface CameraFitProps {
  objUrl: string
}

function CameraFit({ objUrl }: CameraFitProps) {
  const { camera, controls } = useThree()
  const obj = useLoader(OBJLoader, objUrl)

  useEffect(() => {
    const box = new THREE.Box3().setFromObject(obj)
    const center = new THREE.Vector3()
    const size   = new THREE.Vector3()
    box.getCenter(center)
    box.getSize(size)
    const span = Math.max(size.x, size.z)
    camera.position.set(
      center.x - span * 0.6,
      center.y + span * 0.9,
      center.z + span * 1.3,
    )
    camera.lookAt(center)
    if (controls) {
      const orbitControls = controls as unknown as {
        target: THREE.Vector3
        update: () => void
      }
      orbitControls.target.copy(center)
      orbitControls.update()
    }
  }, [obj, camera, controls])

  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────
interface SceneProps {
  jobId: string
  result: RooftopResult
  onPointClick: (point: SamplePoint) => void
}

function Scene({ jobId, result, onPointClick }: SceneProps) {
  const objUrl = `/api/rooftop/${jobId}/terrain.obj`

  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.0} color={0xffeedd} />
      <Suspense fallback={null}>
        <TerrainMesh objUrl={objUrl} displayPoints={result.display_points} />
        <SamplePoints points={result.points} onPointClick={onPointClick} />
        <CameraFit objUrl={objUrl} />
      </Suspense>
    </>
  )
}

// ── Top-level component ───────────────────────────────────────────────────────
export default function RooftopViewer({ jobId, result, onPointClick }: RooftopViewerProps) {
  return (
    <div style={{ position: 'absolute', inset: 0, top: 42 }}>
      <Canvas
        camera={{ fov: 55, near: 0.5, far: 50000 }}
        gl={{ antialias: true }}
        style={{ background: '#111827' }}
      >
        <OrbitControls enableDamping dampingFactor={0.08} />
        {jobId && (
          <Scene jobId={jobId} result={result} onPointClick={onPointClick} />
        )}
      </Canvas>

      {/* Legend */}
      <div style={styles.legend}>
        <div style={styles.legendTitle}>LOS Status</div>
        <LegendRow color="#22cc44" label={`Unobstructed (${result.summary.n_clear})`} />
        <LegendRow color="#ffcc00" label={`Partial (${result.summary.n_partial})`} />
        <LegendRow color="#ff4444" label={`Blocked (${result.summary.n_full})`} />
      </div>
    </div>
  )
}

interface LegendRowProps {
  color: string
  label: string
}

function LegendRow({ color, label }: LegendRowProps) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 7, padding: '3px 0' }}>
      <span style={{
        width: 10, height: 10, borderRadius: '50%',
        background: color, flexShrink: 0,
      }} />
      <span style={{ fontSize: 12, color: '#e5e7eb', fontFamily: 'monospace' }}>{label}</span>
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  legend: {
    position: 'absolute',
    top: 14,
    right: 14,
    background: 'rgba(0,0,0,0.65)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 6,
    padding: '10px 14px',
    pointerEvents: 'none',
  },
  legendTitle: {
    fontSize: 11,
    color: '#6b7280',
    fontFamily: 'monospace',
    textTransform: 'uppercase',
    letterSpacing: '0.07em',
    marginBottom: 7,
  },
}
