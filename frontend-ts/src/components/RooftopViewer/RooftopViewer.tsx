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
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
}

function SamplePoints({ samplePoints, analyses, onPointClick, onRefsChange, onHover, onHoverEnd, hoveredSphereRef }: SamplePointsProps) {
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
          onHover={onHover}
          onHoverEnd={onHoverEnd}
          hoveredSphereRef={hoveredSphereRef}
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
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
}

function SphereGroup({ idxs, samplePoints, color, dummy, hoverable, onPointClick, onMeshMount, onMeshUnmount, onHover, onHoverEnd, hoveredSphereRef }: SphereGroupProps) {
  const meshRef = useRef<THREE.InstancedMesh>(null)
  const geo = useMemo(() => new THREE.SphereGeometry(0.8, 16, 12), [])
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ color }), [color])
  const prevHoveredGlobal = useRef<number>(-1)

  useEffect(() => {
    onMeshMount(meshRef)
    return () => onMeshUnmount(meshRef)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Set all instance matrices at normal scale (positions + scale=1)
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

  // Drive hover scale from the shared ref so it updates in the same frame as the
  // composite shader — eliminating the desync caused by React state → useEffect.
  useFrame(() => {
    const mesh = meshRef.current
    if (!mesh) return
    const cur  = hoveredSphereRef.current
    const prev = prevHoveredGlobal.current
    if (cur === prev) return
    const applyScale = (globalIdx: number, scale: number) => {
      const instanceIdx = idxs.indexOf(globalIdx)
      if (instanceIdx === -1) return
      const mp = samplePoints[globalIdx].measurement_point
      dummy.position.set(mp.x, mp.z, -mp.y)
      dummy.scale.set(scale, scale, scale)
      dummy.updateMatrix()
      mesh.setMatrixAt(instanceIdx, dummy.matrix)
      dummy.scale.set(1, 1, 1)
    }
    if (prev !== -1) applyScale(prev, 1)
    if (cur  !== -1) applyScale(cur,  1.8)
    mesh.instanceMatrix.needsUpdate = true
    prevHoveredGlobal.current = cur
  })

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
          onHover(idxs[e.instanceId])
          document.body.style.cursor = 'pointer'
        }
      } : undefined}
      onPointerOut={hoverable ? () => {
        onHoverEnd()
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

const MAX_SPHERES = 128

interface SphereOutlineProps {
  meshRefs: MeshRef[]
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  hoveredSphereRef: React.MutableRefObject<number>
}

function SphereOutline({ meshRefs, samplePoints, analyses, hoveredSphereRef }: SphereOutlineProps) {
  const { gl, scene, camera, size } = useThree()
  const sphereDataBuf = useRef(new Float32Array(MAX_SPHERES * 4))
  const projVec       = useRef(new THREE.Vector3())
  const edgeVec       = useRef(new THREE.Vector3())
  const camRight      = useRef(new THREE.Vector3())

  useEffect(() => {
    const prevCS = gl.outputColorSpace
    const prevTM = gl.toneMapping
    gl.outputColorSpace = THREE.LinearSRGBColorSpace
    gl.toneMapping      = THREE.NoToneMapping
    return () => { gl.outputColorSpace = prevCS; gl.toneMapping = prevTM }
  }, [gl])

  // ── Render targets ────────────────────────────────────────────────────────────
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

  const whiteMat = useMemo(() => new THREE.MeshBasicMaterial({
    color: 0xffffff,
    depthWrite: false,
  }), [])

  // ── Full-screen composite ─────────────────────────────────────────────────────
  const { orthoScene, orthoCamera, compositeMat } = useMemo(() => {
    const os  = new THREE.Scene()
    const oc  = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
    const mat = new THREE.ShaderMaterial({
      glslVersion: THREE.GLSL3,
      uniforms: {
        sceneColor:  { value: null as THREE.Texture | null },
        sphereMask:  { value: null as THREE.Texture | null },
        resolution:  { value: new THREE.Vector2() },
        uSphereData: { value: new Float32Array(MAX_SPHERES * 4) },
        uNumSpheres: { value: 0 },
        uTime:       { value: 0 },
      },
      vertexShader: /* glsl */`
        out vec2 vUv;
        void main() { vUv = uv; gl_Position = vec4(position, 1.0); }
      `,
      fragmentShader: /* glsl */`
        uniform sampler2D sceneColor;
        uniform sampler2D sphereMask;
        uniform vec2  resolution;
        uniform vec4  uSphereData[${MAX_SPHERES}];
        uniform int   uNumSpheres;
        uniform float uTime;
        in  vec2 vUv;
        out vec4 fragColor;

        float sdSeg(vec2 p, vec2 a, vec2 b) {
          vec2 pa = p - a, ba = b - a;
          return length(pa - ba * clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0));
        }
        // ✓ check mark
        float sdCheck(vec2 p) {
          return min(
            sdSeg(p, vec2(-0.55,  0.05), vec2(-0.10, -0.45)),
            sdSeg(p, vec2(-0.10, -0.45), vec2( 0.60,  0.50))
          );
        }
        // ✗ cross
        float sdCross(vec2 p) {
          return min(
            sdSeg(p, vec2(-0.50, -0.50), vec2(0.50, 0.50)),
            sdSeg(p, vec2(-0.50,  0.50), vec2(0.50, -0.50))
          );
        }
        // ~ tilde (four-segment sine approximation)
        float sdTilde(vec2 p) {
          return min(min(
            sdSeg(p, vec2(-0.55,  0.00), vec2(-0.28,  0.28)),
            sdSeg(p, vec2(-0.28,  0.28), vec2( 0.00,  0.00))
          ), min(
            sdSeg(p, vec2( 0.00,  0.00), vec2( 0.28, -0.28)),
            sdSeg(p, vec2( 0.28, -0.28), vec2( 0.55,  0.00))
          ));
        }
        // ◌ rotating arc spinner (returns alpha directly, not an SDF distance)
        float spinnerAlpha(vec2 p, float t) {
          const float PI = 3.14159265;
          float r      = length(p);
          float radial = smoothstep(0.40, 0.48, r) * smoothstep(0.88, 0.80, r);
          if (radial < 0.001) return 0.0;
          float angle  = mod(atan(p.y, p.x) / (2.0 * PI) - t * 0.7, 1.0);
          float arc    = 0.78;   // fraction of circle that is filled
          float fade   = 0.06;   // angular softness at arc endpoints
          float angA   = smoothstep(0.0, fade, angle) * smoothstep(arc, arc - fade, angle);
          return radial * angA;
        }

        void main() {
          vec4  color = texture(sceneColor, vUv);
          float m     = texture(sphereMask, vUv).r;

          // ── feathered outline ───────────────────────────────────────────────
          float minDist2 = 99.0;
          for (int dx = -3; dx <= 3; dx++) {
            for (int dy = -3; dy <= 3; dy++) {
              float d2 = float(dx * dx + dy * dy);
              if (d2 > 0.0 && d2 <= 9.5) {
                float n = texture(sphereMask, vUv + vec2(float(dx), float(dy)) / resolution).r;
                bool across = (m < 0.5) ? (n > 0.5) : (n < 0.5);
                if (across) minDist2 = min(minDist2, d2);
              }
            }
          }
          float outlineA = (minDist2 < 9.5) ? 1.0 - smoothstep(0.5, 3.5, sqrt(minDist2)) : 0.0;
          vec4 result = mix(color, vec4(1.0), outlineA);

          // ── symbols: ✓ unobstructed · ~ partial · ✗ obstructed · ◌ pending ──
          for (int i = 0; i < ${MAX_SPHERES}; i++) {
            if (i >= uNumSpheres) break;
            vec4  sph    = uSphereData[i];
            int   status = int(sph.z + 0.5);
            if (texture(sphereMask, sph.xy).r < 0.5) continue;
            vec2  delta  = (vUv - sph.xy) * resolution;
            float sr     = sph.w * 0.6;
            if (dot(delta, delta) > (sr + 2.0) * (sr + 2.0)) continue;
            vec2  p = delta / sr;
            float symA;
            if (status == 0) {
              symA = 1.0 - smoothstep(0.0, 6.0 / sr, sdCheck(p));
            } else if (status == 1) {
              symA = 1.0 - smoothstep(0.0, 6.0 / sr, sdTilde(p));
            } else if (status == 2) {
              symA = 1.0 - smoothstep(0.0, 6.0 / sr, sdCross(p));
            } else {
              symA = spinnerAlpha(p, uTime);
            }
            result = mix(result, vec4(1.0), symA);
          }

          fragColor = result;
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

  useFrame((state) => {
    const meshes = meshRefs.map(r => r.current).filter(Boolean) as THREE.InstancedMesh[]
    const ctx = gl.getContext() as WebGL2RenderingContext

    // Pass 1 — render full scene → sceneTgt
    gl.setRenderTarget(targets.sceneTgt)
    gl.render(scene, camera)

    // Pass 2 — sphere mask with HW depth occlusion
    gl.setRenderTarget(targets.maskTgt)
    ctx.colorMask(false, false, false, false)
    gl.render(scene, camera)
    ctx.colorMask(true, true, true, true)
    gl.clear(true, false, false)
    const origLayerMasks = meshes.map(m => m.layers.mask)
    const prevCamMask    = camera.layers.mask
    meshes.forEach(m => m.layers.set(1))
    camera.layers.set(1)
    scene.overrideMaterial = whiteMat
    const prevAutoClear = gl.autoClear
    gl.autoClear = false
    gl.render(scene, camera)
    gl.autoClear = prevAutoClear
    scene.overrideMaterial = null
    camera.layers.mask = prevCamMask
    meshes.forEach((m, i) => { m.layers.mask = origLayerMasks[i] })

    // Project sphere centres to UV space for symbol rendering.
    // Also compute the projected screen radius so symbols scale with the sphere.
    const dpr = gl.getPixelRatio()
    const pw  = Math.round(size.width  * dpr)
    const ph  = Math.round(size.height * dpr)
    const n   = Math.min(samplePoints.length, MAX_SPHERES)
    const buf = sphereDataBuf.current
    const pv  = projVec.current
    const ev  = edgeVec.current
    const cr  = camRight.current
    cr.setFromMatrixColumn(camera.matrixWorld, 0)  // camera right in world space

    for (let i = 0; i < n; i++) {
      const mp = samplePoints[i].measurement_point
      pv.set(mp.x, mp.z, -mp.y)
      pv.project(camera)
      const cx = (pv.x + 1) * 0.5
      const cy = (pv.y + 1) * 0.5

      // Project sphere edge (centre + right × sphere_radius) to get screen radius in px
      ev.set(mp.x + cr.x * 0.8, mp.z + cr.y * 0.8, -mp.y + cr.z * 0.8)
      ev.project(camera)
      const dx = ((ev.x + 1) * 0.5 - cx) * pw
      const dy = ((ev.y + 1) * 0.5 - cy) * ph
      const hoverScale   = i === hoveredSphereRef.current ? 1.8 : 1.0
      const screenRadius = Math.sqrt(dx * dx + dy * dy) * hoverScale

      const result = analyses[i]?.result
      const si     = result === 'unobstructed' ? 0
                   : result === 'partially_obstructed' ? 1
                   : result === 'obstructed' ? 2 : 3
      buf[i * 4 + 0] = cx
      buf[i * 4 + 1] = cy
      buf[i * 4 + 2] = si
      buf[i * 4 + 3] = screenRadius
    }
    compositeMat.uniforms.uSphereData.value = buf
    compositeMat.uniforms.uNumSpheres.value = n
    compositeMat.uniforms.uTime.value = state.clock.elapsedTime

    // Pass 3 — composite to canvas (sRGB output)
    gl.outputColorSpace = THREE.SRGBColorSpace
    gl.setRenderTarget(null)
    gl.render(orthoScene, orthoCamera)
    gl.outputColorSpace = THREE.LinearSRGBColorSpace
  }, 1)

  return null
}

function Scene({ binId, samplePoints, analyses, cameraStateRef, onPointClick }: SceneProps) {
  const objUrl         = `/api/rooftop/render/${binId}`
  const [sphereRefs, setSphereRefs] = useState<MeshRef[]>([])
  const hoveredSphereRef = useRef<number>(-1)
  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.0} color={0xffeedd} />
      <CameraSync stateRef={cameraStateRef} />
      <Suspense fallback={null}>
        <TerrainMesh objUrl={objUrl} samplePoints={samplePoints} analyses={analyses} />
        <SamplePoints
          samplePoints={samplePoints}
          analyses={analyses}
          onPointClick={onPointClick}
          onRefsChange={setSphereRefs}
          onHover={(idx) => { hoveredSphereRef.current = idx }}
          onHoverEnd={() => { hoveredSphereRef.current = -1 }}
          hoveredSphereRef={hoveredSphereRef}
        />
        <CameraFit objUrl={objUrl} stateRef={cameraStateRef} />
      </Suspense>
      <SphereOutline
        meshRefs={sphereRefs}
        samplePoints={samplePoints}
        analyses={analyses}
        hoveredSphereRef={hoveredSphereRef}
      />
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
