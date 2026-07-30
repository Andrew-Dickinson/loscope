/**
 * Full-screen overlay for selecting the far-end antenna position on a rooftop.
 * Loads the building OBJ for the given BIN and lets the user place a single point
 * using the same click + drag placement tool as the main rooftop viewer.
 * On confirm, calls onConfirm with NYS (EPSG:6539) coordinates.
 */
import { useState, useEffect, useCallback, useRef, useMemo, Suspense } from 'react'
import { Canvas, useLoader, useThree } from '@react-three/fiber'
import { OrbitControls, Html, useProgress } from '@react-three/drei'
import * as THREE from 'three'
import { OBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js'
import type { ThreeEvent } from '@react-three/fiber'
import BackgroundTiles from '../RooftopViewer/BackgroundTiles'
import { fetchWithRetry } from '../../lib/fetchWithRetry'

interface FarEndPickerProps {
  binId: string
  label: string
  onConfirm: (nys_e: number, nys_n: number, nys_z: number) => void
  onCancel: () => void
}

// Offset point along face normal: 4 ft on horizontal surfaces, 1 ft on vertical.
function applyNormalOffset(point: THREE.Vector3, face: THREE.Face, object: THREE.Object3D): THREE.Vector3 {
  const worldNormal = face.normal.clone().transformDirection(object.matrixWorld).normalize()
  const offsetFt = 2 + 2 * Math.abs(worldNormal.y)
  return point.clone().addScaledVector(worldNormal, offsetFt)
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
    obj.updateMatrixWorld(true)
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
        const pt = e.face
          ? applyNormalOffset(e.point, e.face, e.object)
          : e.point.clone()
        onPlacementClick(pt)
      }}
    />
  )
}

// ── Placement marker ─────────────────────────────────────────────────────────
// Screen-space billboard: same technique as RooftopViewer's SphereOverlay but
// adapted for a single regular Mesh (modelMatrix instead of instanceMatrix).

const MARKER_VERT = /* glsl */`
  out vec2 vUv;
  void main() {
    vUv = uv;
    float scale   = length(modelMatrix[0].xyz);
    float sphereR = 0.8 * scale;
    vec3  worldCenter = modelMatrix[3].xyz;
    vec4  viewCenter  = viewMatrix * vec4(worldCenter, 1.0);
    vec4  clipCenter  = projectionMatrix * viewCenter;
    float ndcRx = projectionMatrix[0][0] * sphereR / (-viewCenter.z);
    float ndcRy = projectionMatrix[1][1] * sphereR / (-viewCenter.z);
    vec4  clipFront = projectionMatrix * vec4(viewCenter.xyz + vec3(0.0, 0.0, sphereR * 1.05), 1.0);
    gl_Position = vec4(
      clipCenter.x + position.x * ndcRx * 2.6 * clipCenter.w,
      clipCenter.y + position.y * ndcRy * 2.6 * clipCenter.w,
      clipFront.z / clipFront.w * clipCenter.w,
      clipCenter.w
    );
  }
`

const MARKER_FRAG = /* glsl */`
  in  vec2 vUv;
  out vec4 fragColor;
  void main() {
    vec2  p = (vUv - 0.5) * 2.0 * 1.3;
    float r = length(p);
    if (r > 1.15) discard;
    float aa       = fwidth(r);
    float fill     = 1.0 - smoothstep(0.93 - aa, 0.93 + aa, r);
    float ringDist = abs(r - 1.0);
    float ringAA   = fwidth(ringDist);
    float ring     = 1.0 - smoothstep(0.07 - ringAA, 0.07 + ringAA, ringDist);
    float alpha = max(fill, ring);
    if (alpha < 0.01) discard;
    fragColor = vec4(mix(vec3(0.42, 0.13, 0.66), vec3(1.0), ring), alpha);
  }
`

function PlacementMarker({ worldPos, onDragStart }: {
  worldPos: THREE.Vector3
  onDragStart: () => void
}) {
  const mat = useMemo(() => new THREE.ShaderMaterial({
    glslVersion:    THREE.GLSL3,
    transparent:    true,
    depthWrite:     false,
    depthTest:      true,
    vertexShader:   MARKER_VERT,
    fragmentShader: MARKER_FRAG,
  }), [])

  return (
    <group
      position={[worldPos.x, worldPos.y, worldPos.z]}
      onPointerDown={(e: ThreeEvent<PointerEvent>) => { e.stopPropagation(); onDragStart() }}
      onPointerEnter={() => { document.body.style.cursor = 'grab' }}
      onPointerLeave={() => { document.body.style.cursor = 'crosshair' }}
    >
      {/* Invisible hitbox sphere for pointer events */}
      <mesh>
        <sphereGeometry args={[1.04, 16, 12]} />
        <meshBasicMaterial transparent opacity={0} depthWrite={false} />
      </mesh>
      {/* Screen-space billboard quad — scale 1.3 → sphereR = 0.8×1.3 = 1.04 matches hitbox */}
      <mesh scale={[1.3, 1.3, 1.3]}>
        <planeGeometry />
        <primitive object={mat} attach="material" />
      </mesh>
    </group>
  )
}

// ── Camera fit ───────────────────────────────────────────────────────────────

function CameraFit({ objUrl, defaultCamRef, resetCamFnRef }: {
  objUrl: string
  defaultCamRef: React.MutableRefObject<{ pos: THREE.Vector3; target: THREE.Vector3 } | null>
  resetCamFnRef: React.MutableRefObject<(() => void) | null>
}) {
  const { camera, controls, invalidate } = useThree()
  const obj = useLoader(OBJLoader, objUrl)

  useEffect(() => {
    const oc = controls as unknown as { target: THREE.Vector3; update: () => void } | null
    const box = new THREE.Box3().setFromObject(obj)
    const center = new THREE.Vector3(), size = new THREE.Vector3()
    box.getCenter(center); box.getSize(size)
    const span = Math.max(size.x, size.z)
    const pos = new THREE.Vector3(center.x - span * 0.6, center.y + span * 0.9, center.z + span * 1.3)

    // Save default before applying so the CameraChangeTracker 'change' event
    // triggered by oc.update() below correctly sees atDefault=true.
    defaultCamRef.current = { pos: pos.clone(), target: center.clone() }

    resetCamFnRef.current = () => {
      const d = defaultCamRef.current
      if (!d) return
      camera.position.copy(d.pos)
      camera.lookAt(d.target)
      if (oc) { oc.target.copy(d.target); oc.update() }
      invalidate()
    }

    camera.position.copy(pos)
    camera.lookAt(center)
    if (oc) { oc.target.copy(center); oc.update() }
  }, [obj, camera, controls, defaultCamRef, resetCamFnRef, invalidate])

  return null
}

// Listens to OrbitControls 'change' events and reports whether the camera is
// at the default position (set by CameraFit) via onCameraChange.
function CameraChangeTracker({ defaultCamRef, onCameraChange }: {
  defaultCamRef: React.MutableRefObject<{ pos: THREE.Vector3; target: THREE.Vector3 } | null>
  onCameraChange: (atDefault: boolean) => void
}) {
  const { camera, controls, invalidate } = useThree()
  useEffect(() => {
    if (!controls) return
    const oc = controls as unknown as { target: THREE.Vector3; addEventListener: Function; removeEventListener: Function }
    const onChange = () => {
      invalidate()
      const d = defaultCamRef.current
      if (d) {
        // Only consider the orbit target (pan position) — orbiting and zooming
        // move the camera but leave the target unchanged, so the button stays hidden.
        onCameraChange(oc.target.distanceTo(d.target) < 0.01)
      }
    }
    oc.addEventListener('change', onChange)
    return () => oc.removeEventListener('change', onChange)
  }, [camera, controls, defaultCamRef, onCameraChange, invalidate])
  return null
}

// ── Suspense loading fallback (shown while the building OBJ streams in) ───────
function SceneLoadingFallback() {
  const { progress } = useProgress()
  return (
    <Html center>
      <style>{`@keyframes farend-spin { to { transform: rotate(360deg); } }`}</style>
      <div style={st.loadingFallback}>
        <span style={st.loadingSpinner} />
        <span>Loading building{progress > 0 ? ` — ${Math.round(progress)}%` : '…'}</span>
      </div>
    </Html>
  )
}

// ── Scene ─────────────────────────────────────────────────────────────────────

function Scene({ binId, pendingWorldPos, onTerrainClick, buildingOffset, defaultCamRef, resetCamFnRef, onCameraChange }: {
  binId: string
  pendingWorldPos: THREE.Vector3 | null
  onTerrainClick: (point: THREE.Vector3) => void
  buildingOffset: { x_sw: number; y_sw: number } | null
  defaultCamRef: React.MutableRefObject<{ pos: THREE.Vector3; target: THREE.Vector3 } | null>
  resetCamFnRef: React.MutableRefObject<(() => void) | null>
  onCameraChange: (atDefault: boolean) => void
}) {
  const objUrl = `/api/rooftop/render/${binId}`
  const { invalidate, camera, gl, controls } = useThree()

  const isDraggingRef = useRef(false)
  const terrainRef    = useRef<THREE.Object3D | null>(null)
  const raycaster     = useMemo(() => new THREE.Raycaster(), [])

  const handleLoaded = useCallback((obj: THREE.Object3D) => {
    terrainRef.current = obj
  }, [])

  const handleDragStart = useCallback(() => {
    isDraggingRef.current = true
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
      if (!isDraggingRef.current || !terrainRef.current) return
      const rect = canvas.getBoundingClientRect()
      const x =  ((e.clientX - rect.left) / rect.width)  * 2 - 1
      const y = -((e.clientY - rect.top)  / rect.height) * 2 + 1
      raycaster.setFromCamera(new THREE.Vector2(x, y), camera)
      const hits = raycaster.intersectObject(terrainRef.current, true)
      if (hits.length === 0) return
      const hit = hits[0]
      const pt = hit.face
        ? applyNormalOffset(hit.point, hit.face, hit.object)
        : hit.point.clone()
      onTerrainClick(pt)
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
  }, [camera, gl, controls, raycaster, onTerrainClick, invalidate])

  useEffect(() => { invalidate() }, [pendingWorldPos, invalidate])

  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.8} color={0xffeedd} />
      <directionalLight position={[-2, 2, -3]} intensity={0.5} color={0xddeeff} />
      <CameraChangeTracker defaultCamRef={defaultCamRef} onCameraChange={onCameraChange} />
      <Suspense fallback={<SceneLoadingFallback />}>
        <BackgroundTiles binId={binId} buildingOffset={buildingOffset} />
        <TerrainMesh objUrl={objUrl} onPlacementClick={onTerrainClick} onLoaded={handleLoaded} />
        {pendingWorldPos && (
          <PlacementMarker worldPos={pendingWorldPos} onDragStart={handleDragStart} />
        )}
        <CameraFit objUrl={objUrl} defaultCamRef={defaultCamRef} resetCamFnRef={resetCamFnRef} />
      </Suspense>
    </>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────

export default function FarEndPicker({ binId, label, onConfirm, onCancel }: FarEndPickerProps) {
  const [pendingWorldPos, setPendingWorldPos] = useState<THREE.Vector3 | null>(null)
  const [buildingOffset,  setBuildingOffset]  = useState<{ x_sw: number; y_sw: number } | null>(null)
  const [loadError,       setLoadError]       = useState<string | null>(null)
  const [showReset,       setShowReset]       = useState(false)
  const defaultCamRef  = useRef<{ pos: THREE.Vector3; target: THREE.Vector3 } | null>(null)
  const resetCamFnRef  = useRef<(() => void) | null>(null)
  const handleCameraChange = useCallback((atDefault: boolean) => { setShowReset(!atDefault) }, [])

  useEffect(() => {
    let cancelled = false
    fetchWithRetry(`/api/rooftop/samplePoints/${binId}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mast_offset_ft: 0, sample_spacing: 100 }),
    }, () => cancelled)
      .then(r => r.json())
      .then(d => { if (!cancelled) setBuildingOffset({ x_sw: d.sw_offset[0], y_sw: d.sw_offset[1] }) })
      .catch(err => { if (!cancelled) setLoadError(String(err)) })
    return () => { cancelled = true }
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
          {showReset && (
            <button style={st.resetCamBtn} onClick={() => resetCamFnRef.current?.()} title="Reset to default view">
              ↺ Reset view
            </button>
          )}
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
            <Scene
              binId={binId}
              pendingWorldPos={pendingWorldPos}
              onTerrainClick={handleTerrainClick}
              buildingOffset={buildingOffset}
              defaultCamRef={defaultCamRef}
              resetCamFnRef={resetCamFnRef}
              onCameraChange={handleCameraChange}
            />
          </Canvas>
        )}
      </div>
    </div>
  )
}

const BAR_H = 42
const st: Record<string, React.CSSProperties> = {
  loadingFallback: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    whiteSpace: 'nowrap',
    fontSize: 13,
    fontFamily: 'monospace',
    color: '#8b949e',
    background: 'rgba(0,0,0,0.65)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 6,
    padding: '8px 14px',
  },
  loadingSpinner: {
    display: 'inline-block',
    width: 13,
    height: 13,
    borderRadius: '50%',
    border: '2px solid rgba(255,255,255,0.15)',
    borderTopColor: '#4d9fff',
    animation: 'farend-spin 0.7s linear infinite',
    flexShrink: 0,
  },
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
  resetCamBtn: {
    fontSize: 12, fontFamily: 'monospace', color: '#8b949e',
    background: 'rgba(255,255,255,0.05)', border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 4, padding: '3px 10px', cursor: 'pointer',
  },
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
