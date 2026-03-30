/**
 * Interactive 3D rooftop viewer using React Three Fiber.
 *
 * - Loads building OBJ from /api/rooftop/render/<binId>
 * - Colors terrain with Voronoi shader (nearest sample point status)
 * - Renders sample point spheres (grey while analysis pending, colored when done)
 * - Click a sphere → onPointClick(index)
 */
import { useRef, useMemo, useEffect, Suspense } from 'react'
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
  nysB?: [number, number, number] | null
}


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

// ── Direction arrows ──────────────────────────────────────────────────────────
const ARROW_SHAFT_LENGTH = 3.0
const ARROW_HEAD_LENGTH  = 1.0
const ARROW_SHAFT_RADIUS = 0.15
const ARROW_HEAD_RADIUS  = 0.25
const ARROW_OPACITY      = 0.8

const STATUS_COLORS: Record<string, number> = {
  unobstructed:         0x00cc88,
  partially_obstructed: 0xffcc00,
  obstructed:           0xff1a00,
  __pending__:          0x94a3b8,
}

type ArrowEntry = { group: THREE.Group } | null

function DirectionArrows({ samplePoints, analyses, nysB, hoveredSphereRef }: {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  nysB: [number, number, number]
  hoveredSphereRef: React.MutableRefObject<number>
}) {
  const rootRef     = useRef<THREE.Group>(null)
  const entriesRef  = useRef<ArrowEntry[]>([])
  const prevHovered = useRef(-1)


  useEffect(() => {
    const root = rootRef.current
    if (!root) return

    for (const child of [...root.children]) root.remove(child)
    entriesRef.current = []
    prevHovered.current = -1

    const shaftGeo = new THREE.CylinderGeometry(ARROW_SHAFT_RADIUS, ARROW_SHAFT_RADIUS, ARROW_SHAFT_LENGTH, 6)
    const headGeo  = new THREE.ConeGeometry(ARROW_HEAD_RADIUS, ARROW_HEAD_LENGTH, 6)
    const mats: Record<string, THREE.MeshBasicMaterial> = {}
    const getMat = (status: string) => {
      if (!mats[status]) mats[status] = new THREE.MeshBasicMaterial({
        color: STATUS_COLORS[status] ?? STATUS_COLORS.__pending__,
        transparent: true,
        opacity: ARROW_OPACITY,
      })
      return mats[status]
    }
    const yAxis = new THREE.Vector3(0, 1, 0)

    samplePoints.forEach((sp, i) => {
      const mp     = sp.measurement_point
      const status = analyses[i]?.result ?? '__pending__'

      const dx = nysB[0] - mp.nys_e
      const dy = nysB[1] - mp.nys_n
      const dz = nysB[2] - mp.nys_z
      const dir = new THREE.Vector3(dx, dz, -dy)
      if (dir.lengthSq() === 0) { entriesRef.current[i] = null; return }
      dir.normalize()

      const quat    = new THREE.Quaternion().setFromUnitVectors(yAxis, dir)
      const baseMat = getMat(status)

      // Shaft and head positioned relative to the arrow group's local origin
      const shaft = new THREE.Mesh(shaftGeo, baseMat)
      shaft.position.addScaledVector(dir, ARROW_SHAFT_LENGTH / 2)
      shaft.quaternion.copy(quat)

      const head = new THREE.Mesh(headGeo, baseMat)
      head.position.addScaledVector(dir, ARROW_SHAFT_LENGTH + ARROW_HEAD_LENGTH / 2)
      head.quaternion.copy(quat)

      // Group anchored at the measurement point — scaling grows outward from there
      const arrowGroup = new THREE.Group()
      arrowGroup.position.set(mp.x, mp.z, -mp.y)
      arrowGroup.add(shaft)
      arrowGroup.add(head)
      root.add(arrowGroup)

      entriesRef.current[i] = { group: arrowGroup }
    })

    return () => { for (const child of [...root.children]) root.remove(child) }
  }, [samplePoints, analyses, nysB])

  useFrame(() => {
    const cur  = hoveredSphereRef.current
    const prev = prevHovered.current
    if (cur === prev) return

    const apply = (idx: number, hovered: boolean) => {
      const e = entriesRef.current[idx]
      if (!e) return
      e.group.scale.setScalar(hovered ? 1.5 : 1.0)
    }
    if (prev !== -1) apply(prev, false)
    if (cur  !== -1) apply(cur,  true)
    prevHovered.current = cur
  })

  return <group ref={rootRef} />
}

// ── Sample point spheres ──────────────────────────────────────────────────────
interface SamplePointsProps {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  onPointClick: (idx: number) => void
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
}

function SamplePoints({ samplePoints, analyses, onPointClick, onHover, onHoverEnd, hoveredSphereRef }: SamplePointsProps) {
  const dummy = useMemo(() => new THREE.Object3D(), [])

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
          dummy={dummy}
          hoverable={status !== '__pending__'}
          onPointClick={onPointClick}
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
  dummy: THREE.Object3D
  hoverable: boolean
  onPointClick: (idx: number) => void
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
}

function SphereGroup({ idxs, samplePoints, dummy, hoverable, onPointClick, onHover, onHoverEnd, hoveredSphereRef }: SphereGroupProps) {
  const meshRef = useRef<THREE.InstancedMesh>(null)
  const { invalidate } = useThree()
  const geo = useMemo(() => new THREE.SphereGeometry(0.8, 8, 6), [])
  // Invisible — used only for raycasting (click/hover). Visuals come from SphereOverlay.
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ transparent: true, opacity: 0 }), [])
  const prevHoveredGlobal = useRef<number>(-1)

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
          invalidate()
        }
      } : undefined}
      onPointerOut={hoverable ? () => {
        onHoverEnd()
        document.body.style.cursor = 'default'
        invalidate()
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
// Also calls invalidate() so demand-mode canvas re-renders on camera movement.
function CameraSync({ stateRef }: { stateRef: React.MutableRefObject<RooftopCameraState | null> }) {
  const { camera, controls, invalidate } = useThree()
  useEffect(() => {
    if (!controls) return
    const oc = controls as unknown as { target: THREE.Vector3; addEventListener: Function; removeEventListener: Function }
    const save = () => {
      stateRef.current = {
        position: camera.position.toArray() as [number, number, number],
        target:   oc.target.toArray()       as [number, number, number],
      }
      invalidate()
    }
    oc.addEventListener('change', save)
    return () => oc.removeEventListener('change', save)
  }, [camera, controls, stateRef, invalidate])
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
  nysB?: [number, number, number] | null
}

// ── Sphere overlay: billboard ring + SDF symbols, single draw call ────────────
// Replaces the 3-pass render-target pipeline. One InstancedMesh of screen-facing
// quads, depth-tested against the scene — no framebuffer switches.

const OVERLAY_VERT = /* glsl */`
  in float aStatus;
  out vec2  vUv;
  out float vStatus;

  void main() {
    vUv     = uv;
    vStatus = aStatus;

    float scale   = length(instanceMatrix[0].xyz);
    float sphereR = 0.8 * scale;
    vec3  worldCenter = instanceMatrix[3].xyz;

    // View-space sphere centre
    vec4 viewCenter = viewMatrix * vec4(worldCenter, 1.0);

    // Project sphere centre to clip space — this is the exact screen position
    vec4 clipCenter = projectionMatrix * viewCenter;

    // Screen-space sphere radius in NDC, using projection diagonal.
    // projectionMatrix[0][0] = 1/(aspect*tan(fov/2)), [1][1] = 1/tan(fov/2).
    // This correctly handles non-square viewports without distorting the ring.
    float ndcRx = projectionMatrix[0][0] * sphereR / (-viewCenter.z);
    float ndcRy = projectionMatrix[1][1] * sphereR / (-viewCenter.z);

    // Depth: project the sphere front face (1.05× radius toward camera) to get
    // a depth that sits just in front of the sphere surface for the depth test.
    // Crucially this does NOT affect the projected XY — that comes from clipCenter.
    vec4 clipFront = projectionMatrix * vec4(viewCenter.xyz + vec3(0.0, 0.0, sphereR * 1.05), 1.0);

    // Build final clip position entirely in clip/NDC space.
    // XY: sphere centre NDC ± billboard offset (pre-multiplied by w).
    // Z:  sphere front-face depth (converted to this clip.w).
    // W:  sphere centre clip.w.
    gl_Position = vec4(
      clipCenter.x + position.x * ndcRx * 2.6 * clipCenter.w,
      clipCenter.y + position.y * ndcRy * 2.6 * clipCenter.w,
      clipFront.z / clipFront.w * clipCenter.w,
      clipCenter.w
    );
  }
`

const OVERLAY_FRAG = /* glsl */`
  uniform float uTime;
  in  vec2  vUv;
  in  float vStatus;
  out vec4  fragColor;

  float sdSeg(vec2 p, vec2 a, vec2 b) {
    vec2 pa = p - a, ba = b - a;
    return length(pa - ba * clamp(dot(pa,ba)/dot(ba,ba), 0.0, 1.0));
  }
  float sdCheck(vec2 p) {
    return min(sdSeg(p, vec2(-0.55, 0.05), vec2(-0.10,-0.45)),
               sdSeg(p, vec2(-0.10,-0.45), vec2( 0.60, 0.50)));
  }
  float sdCross(vec2 p) {
    return min(sdSeg(p, vec2(-0.50,-0.50), vec2(0.50, 0.50)),
               sdSeg(p, vec2(-0.50, 0.50), vec2(0.50,-0.50)));
  }
  float sdTilde(vec2 p) {
    return min(min(sdSeg(p, vec2(-0.55, 0.00), vec2(-0.28, 0.28)),
                   sdSeg(p, vec2(-0.28, 0.28), vec2( 0.00, 0.00))),
               min(sdSeg(p, vec2( 0.00, 0.00), vec2( 0.28,-0.28)),
                   sdSeg(p, vec2( 0.28,-0.28), vec2( 0.55, 0.00))));
  }
  float spinnerAlpha(vec2 p, float t) {
    const float PI = 3.14159265;
    float r      = length(p);
    float radial = smoothstep(0.40, 0.48, r) * smoothstep(0.88, 0.80, r);
    if (radial < 0.001) return 0.0;
    float angle = mod(atan(p.y, p.x) / (2.0*PI) - t*0.7, 1.0);
    float arc   = 0.78, fade = 0.06;
    return radial * smoothstep(0.0, fade, angle) * smoothstep(arc, arc-fade, angle);
  }

  // Antialiased SDF stroke — AA width derived from actual screen-pixel size via fwidth.
  float sdfStroke(float dist, float halfW) {
    float aa = fwidth(dist);
    return 1.0 - smoothstep(halfW - aa, halfW + aa, dist);
  }

  vec3 statusColor(int s) {
    if (s == 0) return vec3(0.000, 0.800, 0.533);  // unobstructed  teal-green
    if (s == 1) return vec3(1.000, 0.800, 0.000);  // partial       yellow
    if (s == 2) return vec3(1.000, 0.102, 0.000);  // obstructed    red
    return       vec3(0.580, 0.640, 0.720);         // pending       grey
  }

  void main() {
    // p in sphere-radius units (disc edge at r = 1.0, quad edge at 1.3)
    vec2  p = (vUv - 0.5) * 2.0 * 1.3;
    float r = length(p);
    if (r > 1.15) discard;

    int  status  = int(vStatus + 0.5);
    vec3 fillCol = statusColor(status);
    float aa = fwidth(r);

    // Solid fill inside the ring (r < 0.93)
    float fill = 1.0 - smoothstep(0.93 - aa, 0.93 + aa, r);

    // White ring at r = 1.0, half-width 0.07
    float ringDist = abs(r - 1.0);
    float ringAA   = fwidth(ringDist);
    float ring     = 1.0 - smoothstep(0.07 - ringAA, 0.07 + ringAA, ringDist);

    // White symbol inside disc — scaled down 30% by expanding p coords
    float symA = 0.0;
    if (r < 0.93) {
      vec2 ps = p / 0.7;
      if      (status == 0) symA = sdfStroke(sdCheck(ps), 0.06);
      else if (status == 1) symA = sdfStroke(sdTilde(ps), 0.06);
      else if (status == 2) symA = sdfStroke(sdCross(ps), 0.06);
      else                  symA = spinnerAlpha(ps, uTime);
    }

    float alpha = max(fill, ring);
    if (alpha < 0.01) discard;

    // White ring + symbol overlay on top of fill color
    fragColor = vec4(mix(fillCol, vec3(1.0), max(ring, symA)), alpha);
  }
`

function SphereOverlay({ samplePoints, analyses, hoveredSphereRef }: {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null)[]
  hoveredSphereRef: React.MutableRefObject<number>
}) {
  const n = samplePoints.length
  if (n === 0) return null

  // eslint-disable-next-line react-hooks/rules-of-hooks
  const meshRef = useRef<THREE.InstancedMesh>(null)
  // eslint-disable-next-line react-hooks/rules-of-hooks
  const dummy   = useMemo(() => new THREE.Object3D(), [])

  // eslint-disable-next-line react-hooks/rules-of-hooks
  const geo = useMemo(() => {
    const g = new THREE.PlaneGeometry(1, 1)
    g.setAttribute('aStatus', new THREE.InstancedBufferAttribute(new Float32Array(n), 1))
    return g
  }, [n])

  // eslint-disable-next-line react-hooks/rules-of-hooks
  const mat = useMemo(() => new THREE.ShaderMaterial({
    glslVersion:    THREE.GLSL3,
    transparent:    true,
    depthWrite:     false,
    depthTest:      true,
    uniforms:       { uTime: { value: 0 } },
    vertexShader:   OVERLAY_VERT,
    fragmentShader: OVERLAY_FRAG,
  }), [])

  // eslint-disable-next-line react-hooks/rules-of-hooks
  useEffect(() => {
    const mesh = meshRef.current
    if (!mesh) return
    for (let i = 0; i < n; i++) {
      const mp = samplePoints[i].measurement_point
      dummy.position.set(mp.x, mp.z, -mp.y)
      dummy.scale.set(1, 1, 1)
      dummy.updateMatrix()
      mesh.setMatrixAt(i, dummy.matrix)
    }
    mesh.instanceMatrix.needsUpdate = true
  }, [samplePoints, n, dummy])

  const prevHovered = useRef(-1)

  // eslint-disable-next-line react-hooks/rules-of-hooks
  useFrame((state) => {
    const mesh = meshRef.current
    if (!mesh) return

    mat.uniforms.uTime.value = state.clock.elapsedTime

    // Sync status per instance
    const statusAttr = mesh.geometry.getAttribute('aStatus') as THREE.InstancedBufferAttribute
    for (let i = 0; i < n; i++) {
      const r = analyses[i]?.result
      statusAttr.setX(i,
        r === 'unobstructed'         ? 0 :
        r === 'partially_obstructed' ? 1 :
        r === 'obstructed'           ? 2 : 3
      )
    }
    statusAttr.needsUpdate = true

    // Hover scale (mirrors SphereGroup so billboard matches sphere size)
    const cur  = hoveredSphereRef.current
    const prev = prevHovered.current
    if (cur !== prev) {
      const setScale = (idx: number, scale: number) => {
        if (idx < 0 || idx >= n) return
        const mp = samplePoints[idx].measurement_point
        dummy.position.set(mp.x, mp.z, -mp.y)
        dummy.scale.set(scale, scale, scale)
        dummy.updateMatrix()
        mesh.setMatrixAt(idx, dummy.matrix)
        dummy.scale.set(1, 1, 1)
      }
      setScale(prev, 1)
      setScale(cur,  1.8)
      mesh.instanceMatrix.needsUpdate = true
      prevHovered.current = cur
    }

    // Keep rendering while spinner animation is running
    if (analyses.some(a => !a)) state.invalidate()
  })

  return <instancedMesh ref={meshRef} args={[geo, mat, n]} renderOrder={1} />
}

function Scene({ binId, samplePoints, analyses, cameraStateRef, onPointClick, nysB }: SceneProps) {
  const objUrl           = `/api/rooftop/render/${binId}`
  const hoveredSphereRef = useRef<number>(-1)
  const { invalidate }   = useThree()
  useEffect(() => { invalidate() }, [analyses, invalidate])
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
          onHover={(idx) => { hoveredSphereRef.current = idx }}
          onHoverEnd={() => { hoveredSphereRef.current = -1 }}
          hoveredSphereRef={hoveredSphereRef}
        />
        <SphereOverlay
          samplePoints={samplePoints}
          analyses={analyses}
          hoveredSphereRef={hoveredSphereRef}
        />
        {nysB && <DirectionArrows samplePoints={samplePoints} analyses={analyses} nysB={nysB} hoveredSphereRef={hoveredSphereRef} />}
        <CameraFit objUrl={objUrl} stateRef={cameraStateRef} />
      </Suspense>
    </>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────
export default function RooftopViewer({ binId, samplePoints, analyses, cameraStateRef, onPointClick, nysB }: RooftopViewerProps) {
  const n_clear   = analyses.filter(a => a?.result === 'unobstructed').length
  const n_partial = analyses.filter(a => a?.result === 'partially_obstructed').length
  const n_full    = analyses.filter(a => a?.result === 'obstructed').length
  const pending   = analyses.filter(a => a === null).length

  return (
    <div style={{ position: 'absolute', inset: 0, top: 42 }}>
      <Canvas
        camera={{ fov: 55, near: 1, far: 5000 }}
        gl={{ antialias: true }}
        frameloop="demand"
        style={{ background: '#111827' }}
      >
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} />
        <Scene
          binId={binId}
          samplePoints={samplePoints}
          analyses={analyses}
          cameraStateRef={cameraStateRef}
          onPointClick={onPointClick}
          nysB={nysB}
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
