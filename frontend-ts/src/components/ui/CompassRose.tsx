// Floating compass badge for the corner of a 3D view.
//
// Tracks camera yaw only (ignores pitch/roll), like a standard map/CAD
// compass control (Mapbox, Google Maps, SketchUp, etc.) rather than a fully
// 3D orientation gizmo — a flat rotating needle reads far better at a glance
// than a tilting 3D object.
//
// The badge itself is plain DOM, built imperatively and appended next to the
// canvas: R3F's reconciler owns everything under <Canvas>, so a React portal
// from in here can't hand elements to the ReactDOM tree outside it.
//
// Relies on the world convention shared by Tile3DViewer, RooftopViewer, and
// FarEndPicker: +X = easting (East), -Z = northing (North), Y = elevation (up).
import { useEffect, useRef } from 'react'
import { useFrame, useThree } from '@react-three/fiber'
import * as THREE from 'three'

const forward = new THREE.Vector3()

// Base dimensions at scale = 1. The margin and the N-badge's orbit radius
// are scaled together so a shrunk badge still can't clip the viewport edge
// as it swings through a full turn.
const BASE = {
  margin: 40,
  diameter: 88,
  needleInset: 9,
  needleHalfWidth: 10,
  needleLength: 32,
  nBadgeSize: 24,
  nBadgeOrbit: 60,
  centerDot: 8,
  labelFontSize: 13,
}

function styledDiv(style: Partial<CSSStyleDeclaration>) {
  const el = document.createElement('div')
  Object.assign(el.style, style)
  return el
}

/** Renders inside a <Canvas>; has no visual output of its own — instead it
 *  builds and maintains a flat DOM compass badge alongside the canvas.
 *  `bottom`, if given, is a final (unscaled) px value that overrides the
 *  default bottom margin — for stacking above another corner overlay. */
export default function CompassRose({ scale = 1, bottom }: { scale?: number; bottom?: number }) {
  const { gl } = useThree()
  const needleRef = useRef<HTMLDivElement | null>(null)
  const northLabelRef = useRef<HTMLSpanElement | null>(null)

  useEffect(() => {
    const container = gl.domElement.parentElement
    if (!container) return

    const px = (n: number) => `${n * scale}px`

    const badge = styledDiv({
      position: 'absolute',
      right: px(BASE.margin),
      bottom: bottom !== undefined ? `${bottom}px` : px(BASE.margin),
      width: px(BASE.diameter),
      height: px(BASE.diameter),
      borderRadius: '50%',
      background: 'rgba(13, 17, 23, 0.85)',
      border: '1px solid rgba(255, 255, 255, 0.14)',
      boxShadow: '0 1px 4px rgba(0, 0, 0, 0.4)',
      pointerEvents: 'none',
    })

    const needle = styledDiv({ position: 'absolute', inset: '0' })

    const northTri = styledDiv({
      position: 'absolute',
      left: '50%',
      top: px(BASE.needleInset),
      width: '0',
      height: '0',
      marginLeft: px(-BASE.needleHalfWidth),
      borderLeft: `${BASE.needleHalfWidth * scale}px solid transparent`,
      borderRight: `${BASE.needleHalfWidth * scale}px solid transparent`,
      borderBottom: `${BASE.needleLength * scale}px solid #f85149`,
    })

    const southTri = styledDiv({
      position: 'absolute',
      left: '50%',
      bottom: px(BASE.needleInset),
      width: '0',
      height: '0',
      marginLeft: px(-BASE.needleHalfWidth),
      borderLeft: `${BASE.needleHalfWidth * scale}px solid transparent`,
      borderRight: `${BASE.needleHalfWidth * scale}px solid transparent`,
      borderTop: `${BASE.needleLength * scale}px solid #6e7681`,
    })

    // Small badge riding just outside the main circle, in line with the
    // north tip, rather than a label crowding the needle itself.
    const northBadge = styledDiv({
      position: 'absolute',
      left: '50%',
      top: '50%',
      width: px(BASE.nBadgeSize),
      height: px(BASE.nBadgeSize),
      borderRadius: '50%',
      background: 'rgba(13, 17, 23, 0.85)',
      border: '1px solid rgba(255, 255, 255, 0.14)',
      boxShadow: '0 1px 4px rgba(0, 0, 0, 0.4)',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      transform: `translate(-50%, -50%) translateY(${px(-BASE.nBadgeOrbit)})`,
    })

    const label = document.createElement('span')
    Object.assign(label.style, {
      lineHeight: '1',
      fontSize: px(BASE.labelFontSize),
      fontFamily: 'monospace',
      fontWeight: '700',
      color: '#f85149',
    })
    label.textContent = 'N'
    northBadge.appendChild(label)
    northLabelRef.current = label

    const centerDot = styledDiv({
      position: 'absolute',
      left: '50%',
      top: '50%',
      width: px(BASE.centerDot),
      height: px(BASE.centerDot),
      borderRadius: '50%',
      background: '#c9d1d9',
      transform: 'translate(-50%, -50%)',
    })

    needle.append(northTri, southTri, northBadge)
    badge.append(needle, centerDot)
    container.appendChild(badge)
    needleRef.current = needle

    return () => {
      container.removeChild(badge)
      needleRef.current = null
      northLabelRef.current = null
    }
  }, [gl, scale, bottom])

  useFrame(({ camera }) => {
    const el = needleRef.current
    if (!el) return
    camera.getWorldDirection(forward)
    forward.y = 0
    if (forward.lengthSq() < 1e-8) return
    forward.normalize()
    const heading = Math.atan2(forward.x, -forward.z)  // 0 = facing north, +90deg = facing east
    el.style.transform = `rotate(${-heading}rad)`
    // Counter-rotate so the "N" stays upright as the needle (its parent) spins.
    if (northLabelRef.current) northLabelRef.current.style.transform = `rotate(${heading}rad)`
  })

  return null
}
