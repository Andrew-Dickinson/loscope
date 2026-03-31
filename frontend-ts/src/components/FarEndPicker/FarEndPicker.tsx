/**
 * Full-screen overlay for selecting the far-end antenna position on a rooftop.
 * Loads the building OBJ for the given BIN and lets the user place a single point
 * using the same click + drag placement tool as the main rooftop viewer.
 * On confirm, calls onConfirm with NYS (EPSG:6539) coordinates.
 */
import { useState, useEffect, useCallback, useRef, useMemo, Suspense } from 'react'
import { Canvas, useLoader, useThree } from '@react-three/fiber'
import { OrbitControls } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import type { ThreeEvent } from '@react-three/fiber'

interface FarEndPickerProps {
  binId: string
  label: string
  onConfirm: (nys_e: number, nys_n: number, nys_z: number) => void
  onCancel: () => void
}

// ── Terrain mesh ──────────────────────────────────────────────────────────────

function TerrainMesh({ objUrl, onPlacementClick, onLoaded }: {
  objUrl: string
  onPlacementClick: (point: THREE.Vector3) => void
  onLoaded: (obj: THREE.Object3D) => void
}) {
  const obj = useLoader(OBJLoader, objUrl)
  const pointerDownPos = useRef<{ x: number; y: number } | null>(null)
  const mat = useMemo(() => new THREE.MeshLambertMaterial({ color: 0x7a9ab8, side: THREE.DoubleSide }), [])

  useEffect(() => {
    obj.rotation.x = -Math.PI / 2
    obj.traverse(child => {
      if ((child as THREE.Mesh).isMesh) {
        const mesh = child as THREE.Mesh
        mesh.material = mat
        mesh.geometry.computeVertexNormals()
      }
    })
    onLoaded(obj)
  }, [obj, mat, onLoaded])

  return (
    <primitive
      object={obj}
      onPointerDown={(e: ThreeEvent<PointerEvent>) => {
        pointerDownPos.current = { x: e.clientX, y: e.clientY }
      }}
      onClick={(e: ThreeEvent<MouseEvent>) => {
        const down = pointerDownPos.current
        if (!down) return
        const dx = e.clientX - down.x, dy = e.clientY - down.y
        if (dx * dx + dy * dy > 25) return
        e.stopPropagation()
        onPlacementClick(e.point.clone())
      }}
    />
  )
}

// ── Placement marker ─────────────────────────────────────────────────────────

function PlacementMarker({ worldPos, onDragStart }: {
  worldPos: THREE.Vector3
  onDragStart: (tipY: number) => void
}) {
  const mat = useMemo(() => new THREE.MeshBasicMaterial({
    color: 0x4d9fff, transparent: true, opacity: 0.9, depthWrite: false,
  }), [])

  return (
    <mesh
      position={[worldPos.x, worldPos.y, worldPos.z]}
      onPointerDown={(e: ThreeEvent<PointerEvent>) => { e.stopPropagation(); onDragStart(worldPos.y) }}
      onPointerEnter={() => { document.body.style.cursor = 'grab' }}
      onPointerLeave={() => { document.body.style.cursor = 'crosshair' }}
    >
      <sphereGeometry args={[1.04, 16, 12]} />
      <primitive object={mat} attach="material" />
    </mesh>
  )
}

// ── Camera fit ───────────────────────────────────────────────────────────────

function CameraFit({ objUrl }: { objUrl: string }) {
  const { camera, controls } = useThree()
  const obj = useLoader(OBJLoader, objUrl)

  useEffect(() => {
    const oc = controls as unknown as { target: THREE.Vector3; update: () => void } | null
    const box = new THREE.Box3().setFromObject(obj)
    const center = new THREE.Vector3(), size = new THREE.Vector3()
    box.getCenter(center); box.getSize(size)
    const span = Math.max(size.x, size.z)
    camera.position.set(center.x - span * 0.6, center.y + span * 0.9, center.z + span * 1.3)
    camera.lookAt(center)
    if (oc) { oc.target.copy(center); oc.update() }
  }, [obj, camera, controls])

  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────

function Scene({ binId, pendingWorldPos, onTerrainClick }: {
  binId: string
  pendingWorldPos: THREE.Vector3 | null
  onTerrainClick: (point: THREE.Vector3) => void
}) {
  const objUrl = `/api/rooftop/render/${binId}`
  const { invalidate, camera, gl, controls } = useThree()

  const isDraggingRef = useRef(false)
  const dragPlaneYRef = useRef(0)
  const terrainRef    = useRef<THREE.Object3D | null>(null)
  const raycaster     = useMemo(() => new THREE.Raycaster(), [])
  const vertRaycaster = useMemo(() => new THREE.Raycaster(), [])
  const dragPlane     = useMemo(() => new THREE.Plane(new THREE.Vector3(0, 1, 0), 0), [])
  const dragPlaneHit  = useMemo(() => new THREE.Vector3(), [])
  const downDir       = useMemo(() => new THREE.Vector3(0, -1, 0), [])

  const handleLoaded = useCallback((obj: THREE.Object3D) => {
    terrainRef.current = obj
  }, [])

  const handleDragStart = useCallback((tipY: number) => {
    isDraggingRef.current = true
    dragPlaneYRef.current = tipY
    if (controls) (controls as unknown as { enabled: boolean }).enabled = false
    document.body.style.cursor = 'grabbing'
  }, [controls])

  useEffect(() => {
    document.body.style.cursor = 'crosshair'
    return () => { document.body.style.cursor = 'default' }
  }, [])

  useEffect(() => {
    const canvas = gl.domElement

    const onPointerMove = (e: PointerEvent) => {
      if (!isDraggingRef.current) return
      const rect = canvas.getBoundingClientRect()
      const x =  ((e.clientX - rect.left) / rect.width)  * 2 - 1
      const y = -((e.clientY - rect.top)  / rect.height) * 2 + 1
      raycaster.setFromCamera(new THREE.Vector2(x, y), camera)

      dragPlane.constant = -dragPlaneYRef.current
      if (!raycaster.ray.intersectPlane(dragPlane, dragPlaneHit)) return

      vertRaycaster.set(
        new THREE.Vector3(dragPlaneHit.x, dragPlaneHit.y + 500, dragPlaneHit.z),
        downDir,
      )
      const hits = terrainRef.current
        ? vertRaycaster.intersectObject(terrainRef.current, true)
        : []

      onTerrainClick(hits.length > 0
        ? hits[0].point.clone()
        : dragPlaneHit.clone()
      )
      invalidate()
    }

    const onPointerUp = () => {
      if (!isDraggingRef.current) return
      isDraggingRef.current = false
      if (controls) (controls as unknown as { enabled: boolean }).enabled = true
      document.body.style.cursor = 'crosshair'
    }

    canvas.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    return () => {
      canvas.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      isDraggingRef.current = false
      if (controls) (controls as unknown as { enabled: boolean }).enabled = true
    }
  }, [camera, gl, controls, raycaster, vertRaycaster, dragPlane, dragPlaneHit, downDir, onTerrainClick, invalidate])

  useEffect(() => { invalidate() }, [pendingWorldPos, invalidate])

  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.8} color={0xffeedd} />
      <directionalLight position={[-2, 2, -3]} intensity={0.5} color={0xddeeff} />
      <Suspense fallback={null}>
        <TerrainMesh objUrl={objUrl} onPlacementClick={onTerrainClick} onLoaded={handleLoaded} />
        {pendingWorldPos && (
          <PlacementMarker worldPos={pendingWorldPos} onDragStart={handleDragStart} />
        )}
        <CameraFit objUrl={objUrl} />
      </Suspense>
    </>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────

export default function FarEndPicker({ binId, label, onConfirm, onCancel }: FarEndPickerProps) {
  const [pendingWorldPos, setPendingWorldPos] = useState<THREE.Vector3 | null>(null)
  const [buildingOffset,  setBuildingOffset]  = useState<{ x_sw: number; y_sw: number } | null>(null)
  const [loadError,       setLoadError]       = useState<string | null>(null)

  useEffect(() => {
    fetch(`/api/rooftop/samplePoints/${binId}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mast_offset_ft: 0, sample_spacing: 100 }),
    })
      .then(r => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.json() })
      .then(d => setBuildingOffset({ x_sw: d.x_sw, y_sw: d.y_sw }))
      .catch(err => setLoadError(String(err)))
  }, [binId])

  const handleTerrainClick = useCallback((point: THREE.Vector3) => {
    setPendingWorldPos(point)
  }, [])

  const handleConfirm = useCallback(() => {
    if (!pendingWorldPos || !buildingOffset) return
    // Three.js world → NYS: x=easting offset, y=elevation, z=-northing offset
    const nys_e = pendingWorldPos.x + buildingOffset.x_sw
    const nys_n = -pendingWorldPos.z + buildingOffset.y_sw
    const nys_z = pendingWorldPos.y
    onConfirm(nys_e, nys_n, nys_z)
  }, [pendingWorldPos, buildingOffset, onConfirm])

  return (
    <div style={st.overlay}>
      <div style={st.topBar}>
        <div style={st.topLeft}>
          <span style={st.label}>{label}</span>
          <span style={st.hint}>
            {pendingWorldPos
              ? 'Click to reposition · drag to fine-tune'
              : 'Click on the rooftop to place the far-end antenna'}
          </span>
        </div>
        <div style={st.topRight}>
          {pendingWorldPos && (
            <button style={st.confirmBtn} onClick={handleConfirm} disabled={!buildingOffset}>
              Confirm →
            </button>
          )}
          <button style={st.cancelBtn} onClick={onCancel}>Cancel</button>
        </div>
      </div>

      <div style={{ flex: 1, position: 'relative' }}>
        {loadError ? (
          <div style={st.errorMsg}>Failed to load building: {loadError}</div>
        ) : (
          <Canvas
            camera={{ fov: 55, near: 1, far: 5000 }}
            gl={{ antialias: true }}
            frameloop="demand"
            style={{ position: 'absolute', inset: 0, background: '#111827' }}
          >
            <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
            <Scene binId={binId} pendingWorldPos={pendingWorldPos} onTerrainClick={handleTerrainClick} />
          </Canvas>
        )}
      </div>
    </div>
  )
}

const BAR_H = 42
const st: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed', inset: 0, zIndex: 200,
    display: 'flex', flexDirection: 'column',
    background: '#111827',
  },
  topBar: {
    height: BAR_H, flexShrink: 0,
    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
    padding: '0 14px', gap: 12,
    background: 'rgba(13,17,23,0.95)',
    borderBottom: '1px solid rgba(255,255,255,0.07)',
  },
  topLeft:  { display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 },
  topRight: { display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 },
  label: {
    fontSize: 12, fontFamily: 'monospace', color: '#8b949e', flexShrink: 0,
    overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 220,
  },
  hint:    { fontSize: 12, fontFamily: 'monospace', color: '#3d444d' },
  confirmBtn: {
    fontSize: 12, fontFamily: 'monospace', color: '#4d9fff',
    background: 'rgba(77,159,255,0.08)', border: '1px solid rgba(77,159,255,0.4)',
    borderRadius: 4, padding: '3px 10px', cursor: 'pointer',
  },
  cancelBtn: {
    fontSize: 12, fontFamily: 'monospace', color: '#6b7280',
    background: 'transparent', border: 'none', padding: '3px 8px', cursor: 'pointer',
  },
  errorMsg: {
    position: 'absolute', inset: 0,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    fontSize: 13, fontFamily: 'monospace', color: '#ff4444',
  },
}
