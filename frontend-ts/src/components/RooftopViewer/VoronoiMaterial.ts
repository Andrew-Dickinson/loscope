/**
 * Voronoi shader material using a DataTexture to encode sample point positions
 * and LOS statuses. Each pixel is colored by its nearest sample point's status.
 *
 * Uses a Float32 DataTexture (N×1, RGBA) to avoid WebGL uniform array limits:
 *   r = local easting  (Three.js X)
 *   g = elevation ft   (Three.js Y)
 *   b = -local northing (Three.js Z)
 *   a = status int (0=unobstructed, 1=partial, 2=obstructed)
 */
import * as THREE from 'three'

export interface VoronoiPoint {
  x: number  // local easting
  y: number  // local northing
  z: number  // elevation ft
  status: string
}

const STATUS_INT: Record<string, number> = {
  unobstructed: 0,
  partially_obstructed: 1,
  obstructed: 2,
  __pending__: 3,
}

// ShaderMaterial auto-injects: position, normal, modelMatrix, viewMatrix,
// projectionMatrix, normalMatrix — no manual declarations needed.
const VERT = /* glsl */`
  out vec3 vWorld;
  out vec3 vNorm;
  void main() {
    vWorld = (modelMatrix * vec4(position, 1.0)).xyz;
    vNorm  = normalize(normalMatrix * normal);
    gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(position, 1.0);
  }
`

// GLSL ES 3.00 — supports texelFetch + dynamic loop bound.
// With glslVersion: GLSL3, Three.js does NOT inject a gl_FragColor alias,
// so we must declare our own fragment output.
const FRAG = /* glsl */`
uniform sampler2D uPosColTex;
uniform int uNPts;

in vec3 vWorld;
in vec3 vNorm;
out vec4 fragColor;

vec3 statusColor(float s) {
  if (s < 0.5) return vec3(0.000, 0.800, 0.533);  // unobstructed  #00cc88  teal-green
  if (s < 1.5) return vec3(1.000, 0.800, 0.000);  // partial       #ffcc00  yellow
  if (s < 2.5) return vec3(1.000, 0.102, 0.000);  // obstructed    #ff1a00  red
  return       vec3(0.580, 0.640, 0.720);          // pending       grey
}

void main() {
  float bestD2 = 1e15;
  vec3  bestCol = vec3(0.580, 0.640, 0.720);  // fallback grey
  for (int i = 0; i < uNPts; i++) {
    vec4 pd = texelFetch(uPosColTex, ivec2(i, 0), 0);
    vec3 d  = vWorld - pd.xyz;
    float d2 = dot(d, d);
    if (d2 < bestD2) { bestD2 = d2; bestCol = statusColor(pd.w); }
  }
  float diff = max(dot(normalize(vNorm), normalize(vec3(1.0, 3.0, 2.0))), 0.0);
  fragColor = vec4(bestCol * (0.45 + 0.55 * diff), 1.0);
}
`

export function buildVoronoiMaterial(
  displayPoints: VoronoiPoint[] | null | undefined,
): THREE.Material {
  if (!displayPoints || displayPoints.length === 0) {
    return new THREE.MeshLambertMaterial({ color: 0x94a3b8, side: THREE.DoubleSide })
  }

  const N = displayPoints.length
  const data = new Float32Array(N * 4)

  displayPoints.forEach((pt, i) => {
    // Three.js coords: X=east, Y=elevation, Z=-north
    data[i * 4 + 0] = pt.x
    data[i * 4 + 1] = pt.z
    data[i * 4 + 2] = -pt.y
    data[i * 4 + 3] = STATUS_INT[pt.status] ?? 0
  })

  const tex = new THREE.DataTexture(data, N, 1, THREE.RGBAFormat, THREE.FloatType)
  tex.needsUpdate = true

  return new THREE.ShaderMaterial({
    glslVersion: THREE.GLSL3,
    uniforms: {
      uPosColTex: { value: tex },
      uNPts:      { value: N },
    },
    vertexShader:   VERT,
    fragmentShader: FRAG,
    side: THREE.DoubleSide,
  })
}
