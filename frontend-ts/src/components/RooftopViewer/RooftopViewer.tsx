/**
 * Interactive 3D rooftop viewer using React Three Fiber.
 *
 * - Loads building OBJ from /api/rooftop/render/<binId>
 * - Colors terrain with Voronoi shader (nearest sample point status)
 * - Renders sample point spheres (grey while analysis pending, colored when done)
 * - Click a sphere → onPointClick(index)
 */
import { useRef, useMemo, useEffect, useState, Suspense, useCallback } from 'react'
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
  analyses: (PointAnalysis | null | undefined)[]
  cameraStateRef: React.MutableRefObject<RooftopCameraState | null>
  onPointClick: (idx: number) => void
  nysB?: [number, number, number] | null
  mastOffsetFt: number
  buildingOffset: { x_sw: number; y_sw: number } | null
  onAddCustomPoint: (point: BackendSamplePoint) => void
}

// ── Custom-point helpers ──────────────────────────────────────────────────────
function makeCustomPoint(
  worldPos: THREE.Vector3,  // measurement point (mast tip) in Three.js space
  mastOffsetFt: number,
  x_sw: number,
  y_sw: number,
): BackendSamplePoint {
  // Three.js world (after OBJ rotation.x = -PI/2): x=easting, y=elevation, z=-northing
  const local_x   = worldPos.x
  const local_y   = -worldPos.z           // northing
  const meas_z    = worldPos.y            // measurement elevation
  const display_z = meas_z - mastOffsetFt // surface elevation

  const nys_e = local_x + x_sw
  const nys_n = local_y + y_sw

  return {
    display_point:     { x: local_x, y: local_y, z: display_z, nys_e, nys_n, nys_z: display_z },
    measurement_point: { x: local_x, y: local_y, z: meas_z,    nys_e, nys_n, nys_z: meas_z    },
  }
}


// ── Terrain mesh ──────────────────────────────────────────────────────────────
interface TerrainMeshProps {
  objUrl: string
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null | undefined)[]
  placementMode: boolean
  onPlacementClick: (point: THREE.Vector3) => void
  onTerrainLoaded: (obj: THREE.Object3D) => void
}

function TerrainMesh({ objUrl, samplePoints, analyses, placementMode, onPlacementClick, onTerrainLoaded }: TerrainMeshProps) {
  const obj = useLoader(OBJLoader, objUrl)
  const pointerDownPos = useRef<{ x: number; y: number } | null>(null)
  useEffect(() => { onTerrainLoaded(obj) }, [obj, onTerrainLoaded])

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

  useEffect(() => {
    document.body.style.cursor = placementMode ? 'crosshair' : 'default'
    return () => { document.body.style.cursor = 'default' }
  }, [placementMode])

  return (
    <primitive
      object={obj}
      onPointerDown={placementMode ? (e: ThreeEvent<PointerEvent>) => {
        pointerDownPos.current = { x: e.clientX, y: e.clientY }
      } : undefined}
      onClick={placementMode ? (e: ThreeEvent<MouseEvent>) => {
        const down = pointerDownPos.current
        if (!down) return
        const dx = e.clientX - down.x
        const dy = e.clientY - down.y
        if (dx * dx + dy * dy > 25) return
        e.stopPropagation()
        onPlacementClick(e.point.clone())
      } : undefined}
    />
  )
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
  analyses: (PointAnalysis | null | undefined)[]
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

// ── Placement marker ─────────────────────────────────────────────────────────
// worldPos is the measurement point (mast tip) position in Three.js space.
// The surface dot is rendered mastOffsetFt below it.
function PlacementMarker({ worldPos, mastOffsetFt, onDragStart }: {
  worldPos: THREE.Vector3
  mastOffsetFt: number
  onDragStart: (mastTipY: number) => void
}) {
  const mat    = useMemo(() => new THREE.MeshBasicMaterial({ color: 0x4d9fff, transparent: true, opacity: 0.9,  depthWrite: false }), [])
  const dimMat = useMemo(() => new THREE.MeshBasicMaterial({ color: 0x4d9fff, transparent: true, opacity: 0.35, depthWrite: false }), [])

  const mx      = worldPos.x
  const mz      = worldPos.z
  const tipY    = worldPos.y               // mast tip = measurement point
  const surfY   = worldPos.y - mastOffsetFt  // surface below tip

  return (
    <group>
      {/* Surface dot — only shown when there is a mast offset to separate it from the tip */}
      {mastOffsetFt > 0 && (
        <mesh position={[mx, surfY, mz]}>
          <sphereGeometry args={[0.52, 8, 6]} />
          <primitive object={dimMat} attach="material" />
        </mesh>
      )}
      {/* Rod from surface to mast tip */}
      {mastOffsetFt > 0 && (
        <mesh position={[mx, surfY + mastOffsetFt / 2, mz]}>
          <cylinderGeometry args={[0.104, 0.104, mastOffsetFt, 6]} />
          <primitive object={dimMat} attach="material" />
        </mesh>
      )}
      {/* Mast-tip sphere — draggable, sits at cursor position */}
      <mesh
        position={[mx, tipY, mz]}
        onPointerDown={(e: ThreeEvent<PointerEvent>) => { e.stopPropagation(); onDragStart(worldPos.y) }}
        onPointerEnter={() => { document.body.style.cursor = 'grab' }}
        onPointerLeave={() => { document.body.style.cursor = 'crosshair' }}
      >
        <sphereGeometry args={[1.04, 16, 12]} />
        <primitive object={mat} attach="material" />
      </mesh>
    </group>
  )
}

// ── Sample point spheres ──────────────────────────────────────────────────────
interface SamplePointsProps {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null | undefined)[]
  onPointClick: (idx: number) => void
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
  placementMode: boolean
}

function SamplePoints({ samplePoints, analyses, onPointClick, onHover, onHoverEnd, hoveredSphereRef, placementMode }: SamplePointsProps) {
  const dummy = useMemo(() => new THREE.Object3D(), [])

  const groups = useMemo(() => {
    const g: Record<string, number[]> = {}
    samplePoints.forEach((_, i) => {
      const a = analyses[i]
      const status = a === undefined ? '__not_requested__'
                   : a === null      ? '__loading__'
                   : a.result
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
          hoverable={!placementMode && status !== '__loading__'}
          clickable={!placementMode && status !== '__loading__'}
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
  clickable: boolean
  onPointClick: (idx: number) => void
  onHover: (globalIdx: number) => void
  onHoverEnd: () => void
  hoveredSphereRef: React.MutableRefObject<number>
}

function SphereGroup({ idxs, samplePoints, dummy, hoverable, clickable, onPointClick, onHover, onHoverEnd, hoveredSphereRef }: SphereGroupProps) {
  const meshRef  = useRef<THREE.InstancedMesh>(null)
  const depthRef = useRef<THREE.InstancedMesh>(null)
  const { invalidate } = useThree()
  // Hitbox — larger radius for easier interaction, no depth write so it doesn't occlude overlays
  const geo = useMemo(() => new THREE.SphereGeometry(0.96, 8, 6), [])
  const mat = useMemo(() => new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthWrite: false }), [])
  // Depth mask — visual radius, writes depth so circles behind others are occluded correctly
  const depthGeo = useMemo(() => new THREE.SphereGeometry(0.8, 8, 6), [])
  const depthMat = useMemo(() => new THREE.MeshBasicMaterial({ colorWrite: false }), [])
  const prevHoveredGlobal = useRef<number>(-1)
  const pointerDownPos = useRef<{ x: number; y: number } | null>(null)

  useEffect(() => {
    const mesh  = meshRef.current
    const depth = depthRef.current
    if (!mesh) return
    idxs.forEach((ptIdx, i) => {
      const mp = samplePoints[ptIdx].measurement_point
      dummy.position.set(mp.x, mp.z, -mp.y)
      dummy.scale.set(1, 1, 1)
      dummy.updateMatrix()
      mesh.setMatrixAt(i, dummy.matrix)
      depth?.setMatrixAt(i, dummy.matrix)
    })
    mesh.instanceMatrix.needsUpdate = true
    if (depth) depth.instanceMatrix.needsUpdate = true
    // If the hovered point left this group (e.g. transitioned to loading), reset cursor
    if (hoveredSphereRef.current !== -1 && !idxs.includes(hoveredSphereRef.current)) {
      onHoverEnd()
      document.body.style.cursor = 'default'
    }
  }, [idxs, samplePoints, dummy, hoveredSphereRef, onHoverEnd])

  useFrame(() => {
    const mesh  = meshRef.current
    const depth = depthRef.current
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
      depth?.setMatrixAt(instanceIdx, dummy.matrix)
      dummy.scale.set(1, 1, 1)
    }
    if (prev !== -1) applyScale(prev, 1)
    if (cur  !== -1) applyScale(cur,  1.8)
    mesh.instanceMatrix.needsUpdate = true
    if (depth) depth.instanceMatrix.needsUpdate = true
    prevHoveredGlobal.current = cur
  })

  useEffect(() => () => { document.body.style.cursor = 'default' }, [])

  return (
    <>
      <instancedMesh ref={depthRef} args={[depthGeo, depthMat, idxs.length]} />
      <instancedMesh
        ref={meshRef}
        args={[geo, mat, idxs.length]}
        onPointerDown={(e: ThreeEvent<PointerEvent>) => { pointerDownPos.current = { x: e.clientX, y: e.clientY } }}
        onClick={clickable ? (e: ThreeEvent<MouseEvent>) => {
          const down = pointerDownPos.current
          if (!down) return
          const dx = e.clientX - down.x
          const dy = e.clientY - down.y
          if (dx * dx + dy * dy > 25) return  // >5px drag, ignore
          e.stopPropagation()
          if (e.instanceId !== undefined) onPointClick(idxs[e.instanceId])
        } : undefined}
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
    </>
  )
}

// ── Camera state persistence ──────────────────────────────────────────────────
export interface RooftopCameraState {
  binId:    string
  position: [number, number, number]
  target:   [number, number, number]
}

// Writes camera + orbit target to a ref on every controls change (no re-renders).
// Also calls invalidate() so demand-mode canvas re-renders on camera movement.
function CameraSync({ binId, stateRef }: { binId: string; stateRef: React.MutableRefObject<RooftopCameraState | null> }) {
  const { camera, controls, invalidate } = useThree()
  useEffect(() => {
    if (!controls) return
    const oc = controls as unknown as { target: THREE.Vector3; addEventListener: Function; removeEventListener: Function }
    const save = () => {
      stateRef.current = {
        binId,
        position: camera.position.toArray() as [number, number, number],
        target:   oc.target.toArray()       as [number, number, number],
      }
      invalidate()
    }
    oc.addEventListener('change', save)
    return () => oc.removeEventListener('change', save)
  }, [binId, camera, controls, stateRef, invalidate])
  return null
}

// Auto-fits on first load; restores saved state if available.
function CameraFit({ binId, objUrl, stateRef }: { binId: string; objUrl: string; stateRef: React.MutableRefObject<RooftopCameraState | null> }) {
  const { camera, controls } = useThree()
  const obj = useLoader(OBJLoader, objUrl)

  useEffect(() => {
    const oc = controls as unknown as { target: THREE.Vector3; update: () => void } | null
    if (stateRef.current?.binId === binId) {
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
  }, [binId, obj, camera, controls, stateRef])

  return null
}

// ── Scene ─────────────────────────────────────────────────────────────────────
interface SceneProps {
  binId: string
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null | undefined)[]
  cameraStateRef: React.MutableRefObject<RooftopCameraState | null>
  onPointClick: (idx: number) => void
  nysB?: [number, number, number] | null
  placementMode: boolean
  pendingWorldPos: THREE.Vector3 | null
  mastOffsetFt: number
  onPlacementClick: (point: THREE.Vector3) => void
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
      else if (status == 3) symA = spinnerAlpha(ps, uTime);
      // status 4: not yet requested — plain grey circle, no symbol
    }

    float alpha = max(fill, ring);
    if (alpha < 0.01) discard;

    // White ring + symbol overlay on top of fill color
    fragColor = vec4(mix(fillCol, vec3(1.0), max(ring, symA)), alpha);
  }
`

function SphereOverlay({ samplePoints, analyses, hoveredSphereRef }: {
  samplePoints: BackendSamplePoint[]
  analyses: (PointAnalysis | null | undefined)[]
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
      const a = analyses[i]
      const r = a?.result
      statusAttr.setX(i,
        r === 'unobstructed'         ? 0 :
        r === 'partially_obstructed' ? 1 :
        r === 'obstructed'           ? 2 :
        a === null                   ? 3 :  // null = request in flight
                                       4    // undefined = not yet requested
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

    // Keep rendering while spinner animation is running (only for in-flight requests)
    if (analyses.some(a => a === null)) state.invalidate()
  })

  return <instancedMesh ref={meshRef} args={[geo, mat, n]} renderOrder={1} />
}

function Scene({ binId, samplePoints, analyses, cameraStateRef, onPointClick, nysB, placementMode, pendingWorldPos, mastOffsetFt, onPlacementClick }: SceneProps) {
  const objUrl           = `/api/rooftop/render/${binId}`
  const hoveredSphereRef = useRef<number>(-1)
  const { invalidate, camera, gl, controls } = useThree()
  useEffect(() => { invalidate() }, [analyses, invalidate])

  // Drag state for PlacementMarker
  const isDraggingRef  = useRef(false)
  const dragPlaneYRef  = useRef(0)
  const terrainRef     = useRef<THREE.Object3D | null>(null)
  const raycaster      = useMemo(() => new THREE.Raycaster(), [])
  const vertRaycaster  = useMemo(() => new THREE.Raycaster(), [])
  // Horizontal plane reused across frames; constant is set at drag-start
  const dragPlane      = useMemo(() => new THREE.Plane(new THREE.Vector3(0, 1, 0), 0), [])
  const dragPlaneHit   = useMemo(() => new THREE.Vector3(), [])
  const downDir        = useMemo(() => new THREE.Vector3(0, -1, 0), [])

  const handleTerrainLoaded = useCallback((obj: THREE.Object3D) => {
    terrainRef.current = obj
  }, [])

  // Called by PlacementMarker with the current mast-tip Y so we can build the drag plane
  const handleDragStart = useCallback((mastTipY: number) => {
    isDraggingRef.current  = true
    dragPlaneYRef.current  = mastTipY
    if (controls) (controls as unknown as { enabled: boolean }).enabled = false
    document.body.style.cursor = 'grabbing'
  }, [controls])

  useEffect(() => {
    if (!placementMode) return
    const canvas = gl.domElement

    const onPointerMove = (e: PointerEvent) => {
      if (!isDraggingRef.current) return
      const rect = canvas.getBoundingClientRect()
      const x =  ((e.clientX - rect.left) / rect.width)  * 2 - 1
      const y = -((e.clientY - rect.top)  / rect.height) * 2 + 1
      raycaster.setFromCamera(new THREE.Vector2(x, y), camera)

      // Pass 1: horizontal plane at mast-tip Y gives correct XZ without any
      // lateral jump (the cursor is visually at the mast tip, so the plane hit
      // is exactly the grabbed point on the first frame).
      dragPlane.constant = -dragPlaneYRef.current
      if (!raycaster.ray.intersectPlane(dragPlane, dragPlaneHit)) return

      // Pass 2: cast straight down from that XZ to find the actual terrain
      // surface, so the display dot stays on the OBJ rather than a flat plane.
      // Start well above any possible terrain height (+500 ft in Three.js Y).
      vertRaycaster.set(
        new THREE.Vector3(dragPlaneHit.x, dragPlaneHit.y + 500, dragPlaneHit.z),
        downDir,
      )
      const terrainHits = terrainRef.current
        ? vertRaycaster.intersectObject(terrainRef.current, true)
        : []

      if (terrainHits.length > 0) {
        // Surface found — pass the terrain surface point; handleTerrainClick adds mastOffsetFt
        onPlacementClick(terrainHits[0].point.clone())
      } else {
        // Off-building — fall back to plane hit so the marker keeps moving
        onPlacementClick(new THREE.Vector3(dragPlaneHit.x, dragPlaneHit.y - mastOffsetFt, dragPlaneHit.z))
      }
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
  }, [placementMode, mastOffsetFt, camera, gl, controls, raycaster, vertRaycaster, dragPlane, dragPlaneHit, downDir, onPlacementClick, invalidate])

  return (
    <>
      <ambientLight intensity={0.6} />
      <directionalLight position={[1, 3, 2]} intensity={1.0} color={0xffeedd} />
      <CameraSync binId={binId} stateRef={cameraStateRef} />
      <Suspense fallback={null}>
        <TerrainMesh
          objUrl={objUrl}
          samplePoints={samplePoints}
          analyses={analyses}
          placementMode={placementMode}
          onPlacementClick={onPlacementClick}
          onTerrainLoaded={handleTerrainLoaded}
        />
        <SamplePoints
          samplePoints={samplePoints}
          analyses={analyses}
          onPointClick={onPointClick}
          onHover={(idx) => { hoveredSphereRef.current = idx }}
          onHoverEnd={() => { hoveredSphereRef.current = -1 }}
          hoveredSphereRef={hoveredSphereRef}
          placementMode={placementMode}
        />
        <SphereOverlay
          samplePoints={samplePoints}
          analyses={analyses}
          hoveredSphereRef={hoveredSphereRef}
        />
        {nysB && <DirectionArrows samplePoints={samplePoints} analyses={analyses} nysB={nysB} hoveredSphereRef={hoveredSphereRef} />}
        {pendingWorldPos && <PlacementMarker worldPos={pendingWorldPos} mastOffsetFt={mastOffsetFt} onDragStart={handleDragStart} />}
        <CameraFit binId={binId} objUrl={objUrl} stateRef={cameraStateRef} />
      </Suspense>
    </>
  )
}

// ── Top-level ─────────────────────────────────────────────────────────────────
export default function RooftopViewer({ binId, samplePoints, analyses, cameraStateRef, onPointClick, nysB, mastOffsetFt, buildingOffset, onAddCustomPoint }: RooftopViewerProps) {
  const n_clear   = analyses.filter(a => a?.result === 'unobstructed').length
  const n_partial = analyses.filter(a => a?.result === 'partially_obstructed').length
  const n_full    = analyses.filter(a => a?.result === 'obstructed').length
  const pending   = analyses.filter(a => a === null).length

  const [placementMode,   setPlacementMode]   = useState(false)
  const [pendingWorldPos, setPendingWorldPos]  = useState<THREE.Vector3 | null>(null)

  // Store pendingWorldPos as the measurement point (mast tip), not the surface.
  // This way the draggable sphere sits exactly where the cursor is.
  const handleTerrainClick = useCallback((surfacePos: THREE.Vector3) => {
    setPendingWorldPos(new THREE.Vector3(surfacePos.x, surfacePos.y + mastOffsetFt, surfacePos.z))
  }, [mastOffsetFt])

  const handleConfirm = useCallback(() => {
    if (!pendingWorldPos || !buildingOffset) return
    const point = makeCustomPoint(pendingWorldPos, mastOffsetFt, buildingOffset.x_sw, buildingOffset.y_sw)
    onAddCustomPoint(point)
    setPlacementMode(false)
    setPendingWorldPos(null)
  }, [pendingWorldPos, mastOffsetFt, buildingOffset, onAddCustomPoint])

  const handleCancel = useCallback(() => {
    setPlacementMode(false)
    setPendingWorldPos(null)
  }, [])

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
          placementMode={placementMode}
          pendingWorldPos={pendingWorldPos}
          mastOffsetFt={mastOffsetFt}
          onPlacementClick={handleTerrainClick}
        />
      </Canvas>

      <div style={styles.legend}>
        <div style={styles.legendTitle}>LOS Status</div>
        <LegendRow color="#00cc88" label={`Unobstructed (${n_clear})`} />
        <LegendRow color="#ffcc00" label={`Partial (${n_partial})`} />
        <LegendRow color="#ff1a00" label={`Obstructed (${n_full})`} />
        {pending > 0 && <LegendRow color="#94a3b8" label={`Pending (${pending})`} />}
      </div>

      <div style={styles.placementPanel}>
        {!placementMode ? (
          <button
            style={styles.addBtn}
            onClick={() => setPlacementMode(true)}
            title="Add a custom sample point"
          >
            + Add point
          </button>
        ) : (
          <>
            <span style={styles.placementHint}>
              {pendingWorldPos ? 'Click to reposition · ' : 'Click rooftop to place · '}
            </span>
            {pendingWorldPos && (
              <button style={{ ...styles.addBtn, ...styles.confirmBtn }} onClick={handleConfirm}>
                Confirm
              </button>
            )}
            <button style={styles.cancelBtn} onClick={handleCancel}>
              Cancel
            </button>
          </>
        )}
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
  placementPanel: {
    position: 'absolute',
    bottom: 14,
    left: 14,
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    background: 'rgba(0,0,0,0.65)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 6,
    padding: '6px 10px',
  },
  addBtn: {
    fontSize: 12,
    fontFamily: 'monospace',
    color: '#8b949e',
    background: '#21262d',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 4,
    padding: '2px 9px',
    cursor: 'pointer',
  },
  confirmBtn: {
    color: '#4d9fff',
    borderColor: 'rgba(77,159,255,0.4)',
    background: 'rgba(77,159,255,0.08)',
  },
  cancelBtn: {
    fontSize: 12,
    fontFamily: 'monospace',
    color: '#6b7280',
    background: 'transparent',
    border: 'none',
    padding: '2px 6px',
    cursor: 'pointer',
  },
  placementHint: {
    fontSize: 12,
    fontFamily: 'monospace',
    color: '#4d9fff',
    userSelect: 'none',
  },
}
