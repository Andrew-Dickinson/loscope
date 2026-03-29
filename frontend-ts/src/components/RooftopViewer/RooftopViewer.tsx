/**
 * Interactive 3D rooftop viewer using React Three Fiber.
 *
 * - Loads building OBJ from /api/rooftop/render/<binId>
 * - Colors terrain with Voronoi shader (nearest sample point status)
 * - Renders sample point spheres (grey while analysis pending, colored when done)
 * - Click a sphere → onPointClick(index)
 */
import { useRef, useState, useMemo, useEffect, Suspense } from 'react'
import { Canvas, useLoader, useThree } from '@react-three/fiber'
import { OrbitControls } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import { buildVoronoiMaterial } from './VoronoiMaterial'
import type { ThreeEvent } from '@react-three/fiber'

export interface EncodedPoint {
  x: number
  y: number
  z: number
  nys_e: number
  nys_n: number
  nys_z: number
}

export interface BackendSamplePoint {
  display_point: EncodedPoint
  measurement_point: EncodedPoint
}

export interface PointAnalysis {
  analysis_id: string
  result: string  // 'unobstructed' | 'partially_obstructed' | 'obstructed'
}

interface RooftopViewerProps {
  binId: string
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  cameraStateRef: React.MutableRefObject<RooftopCameraState | null>
  onPointClick: (idx: number) => void
}

const STATUS_COLOR: Record<string, number> = {
  unobstructed:         0x22cc44,
  partially_obstructed: 0xffcc00,
  obstructed:           0xff4444,
}
const PENDING_COLOR = 0x94a3b8

// ── Terrain mesh ──────────────────────────────────────────────────────────────
interface TerrainMeshProps {
  objUrl: string
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
}

function TerrainMesh({ objUrl, samplePoints, analyses }: TerrainMeshProps) {
  const obj = useLoader(OBJLoader, objUrl)

  const voronoiPoints = useMemo(() => samplePoints.map((sp, i) => ({
    x: sp.display_point.x,
    y: sp.display_point.y,
    z: sp.display_point.z,
    status: analyses[i]?.result ?? '__pending__',
  })), [samplePoints, analyses])

  const mat = useMemo(() => buildVoronoiMaterial(voronoiPoints), [voronoiPoints])

  useEffect(() => {
    // OBJ is Z-up (X=easting, Y=northing, Z=elev); Three.js is Y-up.
    // Rotate -90° around X → maps (x,y,z) to (x, z, -y), matching sphere placement.
    obj.rotation.x = -Math.PI / 2
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
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  onPointClick: (idx: number) => void
}

function SamplePoints({ samplePoints, analyses, onPointClick }: SamplePointsProps) {
  const dummy = useMemo(() => new THREE.Object3D(), [])

  // Group indices by color
  const groups = useMemo(() => {
    const g: Record<string, number[]> = {}
    samplePoints.forEach((_, i) => {
      const status = analyses[i]?.result ?? '__pending__'
      if (!g[status]) g[status] = []
      g[status].push(i)
    })
    return g
  }, [samplePoints, analyses])

  return (
    <>
      {Object.entries(groups).map(([status, idxs]) => (
        <SphereGroup
          key={status}
          idxs={idxs}
          samplePoints={samplePoints}
          color={STATUS_COLOR[status] ?? PENDING_COLOR}
          dummy={dummy}
          hoverable={status !== '__pending__'}
          onPointClick={onPointClick}
        />
      ))}
    </>
  )
}

interface SphereGroupProps {
  idxs: number[]
  samplePoints: BackendSamplePoint[]
  color: number
  dummy: THREE.Object3D
  hoverable: boolean
  onPointClick: (idx: number) => void
}

function SphereGroup({ idxs, samplePoints, color, dummy, hoverable, onPointClick }: SphereGroupProps) {
  const meshRef = useRef<THREE.InstancedMesh>(null)
  const geo = useMemo(() => new THREE.SphereGeometry(0.8, 8, 6), [])
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ color }), [color])
  const [hoveredId, setHoveredId] = useState<number | null>(null)
  const prevHoveredId = useRef<number | null>(null)

  // Set all instance matrices at normal scale
  useEffect(() => {
    const mesh = meshRef.current
    if (!mesh) return
    idxs.forEach((ptIdx, i) => {
      const mp = samplePoints[ptIdx].measurement_point
      dummy.position.set(mp.x, mp.z, -mp.y)
      dummy.scale.set(1, 1, 1)
      dummy.updateMatrix()
      mesh.setMatrixAt(i, dummy.matrix)
    })
    mesh.instanceMatrix.needsUpdate = true
  }, [idxs, samplePoints, dummy])

  // Scale hovered instance up, restore previous
  useEffect(() => {
    const mesh = meshRef.current
    if (!mesh) return
    const applyScale = (instanceIdx: number, scale: number) => {
      const mp = samplePoints[idxs[instanceIdx]].measurement_point
      dummy.position.set(mp.x, mp.z, -mp.y)
      dummy.scale.set(scale, scale, scale)
      dummy.updateMatrix()
      mesh.setMatrixAt(instanceIdx, dummy.matrix)
      dummy.scale.set(1, 1, 1)
    }
    if (prevHoveredId.current !== null) applyScale(prevHoveredId.current, 1)
    if (hoveredId !== null) applyScale(hoveredId, 1.8)
    prevHoveredId.current = hoveredId
    mesh.instanceMatrix.needsUpdate = true
  }, [hoveredId, idxs, samplePoints, dummy])

  // Reset cursor if unmounted while hovering
  useEffect(() => () => { document.body.style.cursor = 'default' }, [])

  return (
    <instancedMesh
      ref={meshRef}
      args={[geo, mat, idxs.length]}
      onClick={(e: ThreeEvent<MouseEvent>) => {
        e.stopPropagation()
        if (e.instanceId !== undefined) onPointClick(idxs[e.instanceId])
      }}
      onPointerOver={hoverable ? (e: ThreeEvent<PointerEvent>) => {
        e.stopPropagation()
        if (e.instanceId !== undefined) {
          setHoveredId(e.instanceId)
          document.body.style.cursor = 'pointer'
        }
      } : undefined}
      onPointerOut={hoverable ? () => {
        setHoveredId(null)
        document.body.style.cursor = 'default'
      } : undefined}
    />
  )
}

// ── Camera state persistence ──────────────────────────────────────────────────
export interface RooftopCameraState {
  position: [number, number, number]
  target:   [number, number, number]
}

// Writes camera + orbit target to a ref on every controls change (no re-renders).
function CameraSync({ stateRef }: { stateRef: React.MutableRefObject<RooftopCameraState | null> }) {
  const { camera, controls } = useThree()
  useEffect(() => {
    if (!controls) return
    const oc = controls as unknown as { target: THREE.Vector3; addEventListener: Function; removeEventListener: Function }
    const save = () => {
      stateRef.current = {
        position: camera.position.toArray() as [number, number, number],
        target:   oc.target.toArray()       as [number, number, number],
      }
    }
    oc.addEventListener('change', save)
    return () => oc.removeEventListener('change', save)
  }, [camera, controls, stateRef])
  return null
}

// Auto-fits on first load; restores saved state if available.
function CameraFit({ objUrl, stateRef }: { objUrl: string; stateRef: React.MutableRefObject<RooftopCameraState | null> }) {
  const { camera, controls } = useThree()
  const obj = useLoader(OBJLoader, objUrl)

  useEffect(() => {
    const oc = controls as unknown as { target: THREE.Vector3; update: () => void } | null
    if (stateRef.current) {
      const { position, target } = stateRef.current
      camera.position.set(...position)
      if (oc) { oc.target.set(...target); oc.update() }
      camera.lookAt(new THREE.Vector3(...target))
    } else {
      const box = new THREE.Box3().setFromObject(obj)
      const center = new THREE.Vector3()
      const size   = new THREE.Vector3()
      box.getCenter(center)
      box.getSize(size)
      const span = Math.max(size.x, size.z)
      camera.position.set(center.x - span * 0.6, center.y + span * 0.9, center.z + span * 1.3)
      camera.lookAt(center)
      if (oc) { oc.target.copy(center); oc.update() }
    }
  }, [obj, camera, controls, stateRef])

  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────
interface SceneProps {
  binId: string
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  cameraStateRef: React.MutableRefObject<RooftopCameraState | null>
  onPointClick: (idx: number) => void
}

function Scene({ binId, samplePoints, analyses, cameraStateRef, onPointClick }: SceneProps) {
  const objUrl = `/api/rooftop/render/${binId}`
  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.0} color={0xffeedd} />
      <CameraSync stateRef={cameraStateRef} />
      <Suspense fallback={null}>
        <TerrainMesh objUrl={objUrl} samplePoints={samplePoints} analyses={analyses} />
        <SamplePoints samplePoints={samplePoints} analyses={analyses} onPointClick={onPointClick} />
        <CameraFit objUrl={objUrl} stateRef={cameraStateRef} />
      </Suspense>
    </>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────
export default function RooftopViewer({ binId, samplePoints, analyses, cameraStateRef, onPointClick }: RooftopViewerProps) {
  const n_clear   = analyses.filter(a => a?.result === 'unobstructed').length
  const n_partial = analyses.filter(a => a?.result === 'partially_obstructed').length
  const n_full    = analyses.filter(a => a?.result === 'obstructed').length
  const pending   = analyses.filter(a => a === null).length

  return (
    <div style={{ position: 'absolute', inset: 0, top: 42 }}>
      <Canvas
        camera={{ fov: 55, near: 0.5, far: 50000 }}
        gl={{ antialias: true }}
        style={{ background: '#111827' }}
      >
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Scene
          binId={binId}
          samplePoints={samplePoints}
          analyses={analyses}
          cameraStateRef={cameraStateRef}
          onPointClick={onPointClick}
        />
      </Canvas>

      <div style={styles.legend}>
        <div style={styles.legendTitle}>LOS Status</div>
        <LegendRow color="#22cc44" label={`Unobstructed (${n_clear})`} />
        <LegendRow color="#ffcc00" label={`Partial (${n_partial})`} />
        <LegendRow color="#ff4444" label={`Obstructed (${n_full})`} />
        {pending > 0 && <LegendRow color="#94a3b8" label={`Pending (${pending})`} />}
      </div>
    </div>
  )
}

function LegendRow({ color, label }: { color: string; label: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 7, padding: '3px 0' }}>
      <span style={{ width: 10, height: 10, borderRadius: '50%', background: color, flexShrink: 0 }} />
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
