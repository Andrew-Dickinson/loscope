/**
 * Interactive 3D rooftop viewer using React Three Fiber.
 *
 * - Loads building OBJ from /api/rooftop/render/<binId>
 * - Colors terrain with Voronoi shader (nearest sample point status)
 * - Renders sample point spheres (grey while analysis pending, colored when done)
 * - Click a sphere → onPointClick(index)
 */
import { useRef, useState, useMemo, useEffect, useCallback, Suspense } from 'react'
import { Canvas, useLoader, useThree, useFrame } from '@react-three/fiber'
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
  unobstructed:         0x00cc88,  // teal-green  (deuteranopia: blue-teal, distinct from orange)
  partially_obstructed: 0xffcc00,  // yellow      (unchanged — already deuteranopia-safe)
  obstructed:           0xff1a00,  // red         (deuteranopia: amber, distinct from teal)
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
type MeshRef = React.MutableRefObject<THREE.InstancedMesh | null>

interface SamplePointsProps {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  onPointClick: (idx: number) => void
  onRefsChange: (refs: MeshRef[]) => void
}

function SamplePoints({ samplePoints, analyses, onPointClick, onRefsChange }: SamplePointsProps) {
  const dummy = useMemo(() => new THREE.Object3D(), [])
  const refsRef = useRef<MeshRef[]>([])

  const addRef = useCallback((ref: MeshRef) => {
    refsRef.current = [...refsRef.current, ref]
    onRefsChange(refsRef.current)
  }, [onRefsChange])

  const removeRef = useCallback((ref: MeshRef) => {
    refsRef.current = refsRef.current.filter(r => r !== ref)
    onRefsChange(refsRef.current)
  }, [onRefsChange])

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
          onMeshMount={addRef}
          onMeshUnmount={removeRef}
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
  onMeshMount: (ref: MeshRef) => void
  onMeshUnmount: (ref: MeshRef) => void
}

function SphereGroup({ idxs, samplePoints, color, dummy, hoverable, onPointClick, onMeshMount, onMeshUnmount }: SphereGroupProps) {
  const meshRef = useRef<THREE.InstancedMesh>(null)
  const geo = useMemo(() => new THREE.SphereGeometry(0.8, 16, 12), [])
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ color }), [color])
  const [hoveredId, setHoveredId] = useState<number | null>(null)
  const prevHoveredId = useRef<number | null>(null)

  // Register the ref (not the mesh) — ref.current stays valid even when R3F
  // recreates the InstancedMesh due to args changes (e.g. idxs.length changes)
  useEffect(() => {
    onMeshMount(meshRef)
    return () => onMeshUnmount(meshRef)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

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

function SphereOutline({ meshRefs }: { meshRefs: MeshRef[] }) {
  const { gl, scene, camera, size } = useThree()

  // Keep linear colour space while mounted so R3F's own render (priority 0) doesn't
  // apply sRGB encoding. Our composite pass does the single final sRGB conversion.
  useEffect(() => {
    const prevCS = gl.outputColorSpace
    const prevTM = gl.toneMapping
    gl.outputColorSpace = THREE.LinearSRGBColorSpace
    gl.toneMapping      = THREE.NoToneMapping
    return () => { gl.outputColorSpace = prevCS; gl.toneMapping = prevTM }
  }, [gl])

  // ── Render targets ────────────────────────────────────────────────────────────
  // Both targets keep a depth buffer.  maskTgt reuses its own depth for HW occlusion.
  // Must use physical pixels (CSS size × DPR) or the scene renders at CSS resolution.
  const targets = useMemo(() => {
    const dpr = gl.getPixelRatio()
    const w = Math.round(size.width  * dpr)
    const h = Math.round(size.height * dpr)
    const sceneTgt = new THREE.WebGLRenderTarget(w, h)
    const maskTgt  = new THREE.WebGLRenderTarget(w, h)
    return { sceneTgt, maskTgt }
  }, [gl, size.width, size.height])

  useEffect(() => () => {
    targets.sceneTgt.dispose()
    targets.maskTgt.dispose()
  }, [targets])

  // White material for the sphere-only mask render.
  // MeshBasicMaterial handles InstancedMesh natively; depthTest gives HW occlusion.
  const whiteMat = useMemo(() => new THREE.MeshBasicMaterial({
    color: 0xffffff,
    depthWrite: false,
  }), [])

  // ── Full-screen composite ─────────────────────────────────────────────────────
  const { orthoScene, orthoCamera, compositeMat } = useMemo(() => {
    const os  = new THREE.Scene()
    const oc  = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
    const mat = new THREE.ShaderMaterial({
      uniforms: {
        sceneColor: { value: null as THREE.Texture | null },
        sphereMask: { value: null as THREE.Texture | null },
        resolution:  { value: new THREE.Vector2() },
      },
      vertexShader: /* glsl */`
        varying vec2 vUv;
        void main() { vUv = uv; gl_Position = vec4(position, 1.0); }
      `,
      fragmentShader: /* glsl */`
        uniform sampler2D sceneColor;
        uniform sampler2D sphereMask;
        uniform vec2 resolution;
        varying vec2 vUv;
        void main() {
          vec4  color = texture2D(sceneColor, vUv);
          float m     = texture2D(sphereMask, vUv).r;
          // Find distance to the nearest pixel with the OPPOSITE mask state.
          // This feathers symmetrically: outside pixels fade in toward the edge,
          // inside pixels fade in toward the edge, creating a smooth glow on both sides.
          float minDist2 = 99.0;
          for (int dx = -3; dx <= 3; dx++) {
            for (int dy = -3; dy <= 3; dy++) {
              float d2 = float(dx * dx + dy * dy);
              if (d2 > 0.0 && d2 <= 9.5) {
                float n = texture2D(
                  sphereMask, vUv + vec2(float(dx), float(dy)) / resolution
                ).r;
                bool acrossBoundary = (m < 0.5) ? (n > 0.5) : (n < 0.5);
                if (acrossBoundary) minDist2 = min(minDist2, d2);
              }
            }
          }
          float alpha = (minDist2 < 9.5) ? 1.0 - smoothstep(0.5, 3.5, sqrt(minDist2)) : 0.0;
          gl_FragColor = mix(color, vec4(1.0), alpha);
        }
      `,
    })
    const quad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), mat)
    quad.frustumCulled = false
    os.add(quad)
    return { orthoScene: os, orthoCamera: oc, compositeMat: mat }
  }, [])

  useEffect(() => {
    const dpr = gl.getPixelRatio()
    compositeMat.uniforms.sceneColor.value = targets.sceneTgt.texture
    compositeMat.uniforms.sphereMask.value = targets.maskTgt.texture
    compositeMat.uniforms.resolution.value.set(
      Math.round(size.width  * dpr),
      Math.round(size.height * dpr),
    )
  }, [gl, targets, size, compositeMat])

  useFrame(() => {
    const meshes = meshRefs.map(r => r.current).filter(Boolean) as THREE.InstancedMesh[]
    const ctx = gl.getContext() as WebGL2RenderingContext

    // Pass 1 — render full scene to sceneTgt (colour + depth populated by HW)
    gl.setRenderTarget(targets.sceneTgt)
    gl.render(scene, camera)

    // Pass 2 — build sphere mask in maskTgt with correct HW depth occlusion:
    //   2a: render full scene to maskTgt with colour writes off → populates depth buffer
    gl.setRenderTarget(targets.maskTgt)
    ctx.colorMask(false, false, false, false)
    gl.render(scene, camera)
    ctx.colorMask(true, true, true, true)

    //   2b: clear only the colour buffer, keeping the depth from 2a
    gl.clear(true, false, false)

    //   2c: render sphere-only layer with HW depth test; occluded fragments are
    //       naturally discarded because their depth > the scene depth in the buffer.
    const origLayerMasks = meshes.map(m => m.layers.mask)
    const prevCamMask    = camera.layers.mask
    meshes.forEach(m => m.layers.set(1))
    camera.layers.set(1)
    scene.overrideMaterial = whiteMat
    const prevAutoClear = gl.autoClear
    gl.autoClear = false       // must NOT clear depth before this render
    gl.render(scene, camera)
    gl.autoClear = prevAutoClear
    scene.overrideMaterial = null
    camera.layers.mask = prevCamMask
    meshes.forEach((m, i) => { m.layers.mask = origLayerMasks[i] })

    // Pass 3 — composite scene colour + outline onto the canvas (sRGB output)
    gl.outputColorSpace = THREE.SRGBColorSpace
    gl.setRenderTarget(null)
    gl.render(orthoScene, orthoCamera)
    gl.outputColorSpace = THREE.LinearSRGBColorSpace  // restore for next frame
  }, 1)

  return null
}

function Scene({ binId, samplePoints, analyses, cameraStateRef, onPointClick }: SceneProps) {
  const objUrl = `/api/rooftop/render/${binId}`
  const [sphereRefs, setSphereRefs] = useState<MeshRef[]>([])
  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.0} color={0xffeedd} />
      <CameraSync stateRef={cameraStateRef} />
      <Suspense fallback={null}>
        <TerrainMesh objUrl={objUrl} samplePoints={samplePoints} analyses={analyses} />
        <SamplePoints samplePoints={samplePoints} analyses={analyses} onPointClick={onPointClick} onRefsChange={setSphereRefs} />
        <CameraFit objUrl={objUrl} stateRef={cameraStateRef} />
      </Suspense>
      <SphereOutline meshRefs={sphereRefs} />
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
        camera={{ fov: 55, near: 1, far: 5000 }}
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
        <LegendRow color="#00cc88" label={`Unobstructed (${n_clear})`} />
        <LegendRow color="#ffcc00" label={`Partial (${n_partial})`} />
        <LegendRow color="#ff1a00" label={`Obstructed (${n_full})`} />
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
