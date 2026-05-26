/**
 * Background terrain tiles for the rooftop 3D views.
 *
 * Fetches all subgrid tiles whose footprint intersects the building footprint,
 * then renders each as a displaced PlaneGeometry with ortho texture. The backend
 * returns heightmaps with footprint pixels zeroed so the background geometry
 * never overlaps the rooftop OBJ.
 *
 * Coordinate system matches RooftopViewer / FarEndPicker:
 *   X = local easting  (relative to building SW corner)
 *   Y = elevation (ft)
 *   Z = -local northing
 */
import { useEffect, useState } from 'react'
import * as THREE from 'three'

interface TileInfo {
  id: string
  sw_nys: [number, number]  // [easting, northing]
}

interface BackgroundTilesProps {
  binId: string
  buildingOffset: { x_sw: number; y_sw: number } | null
}

export default function BackgroundTiles({ binId, buildingOffset }: BackgroundTilesProps) {
  const [tiles, setTiles] = useState<TileInfo[]>([])

  useEffect(() => {
    let cancelled = false
    fetch(`/api/rooftop/backgroundTileIds/${binId}`)
      .then(r => r.ok ? r.json() : Promise.reject(r.status))
      .then((data: { tiles: TileInfo[] }) => { if (!cancelled) setTiles(data.tiles) })
      .catch(() => {})
    return () => { cancelled = true }
  }, [binId])

  if (!buildingOffset || tiles.length === 0) return null

  return (
    <>
      {tiles.map(tile => (
        <BackgroundTile
          key={tile.id}
          binId={binId}
          tileId={tile.id}
          swNys={tile.sw_nys}
          buildingOffset={buildingOffset}
        />
      ))}
    </>
  )
}

// ── Single tile ───────────────────────────────────────────────────────────────

interface BackgroundTileProps {
  binId: string
  tileId: string
  swNys: [number, number]
  buildingOffset: { x_sw: number; y_sw: number }
}

function BackgroundTile({ binId, tileId, swNys, buildingOffset }: BackgroundTileProps) {
  const [geometry, setGeometry] = useState<THREE.BufferGeometry | null>(null)
  const [texture,  setTexture]  = useState<THREE.Texture | null>(null)

  // Build displaced PlaneGeometry from masked heightmap TIFF
  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        const resp = await fetch(`/api/rooftop/backgroundTileRaster/${binId}/${tileId}`)
        if (!resp.ok || cancelled) return
        const buf = await resp.arrayBuffer()
        if (cancelled) return

        const { fromArrayBuffer } = await import('geotiff')
        const tiff = await fromArrayBuffer(buf)
        const image = await tiff.getImage()
        const rasters = await image.readRasters()
        const data = rasters[0] as Uint16Array
        const width  = image.getWidth()   // northing count (TIFF cols)
        const height = image.getHeight()  // easting count  (TIFF rows)

        const geo = new THREE.PlaneGeometry(500, 500, width - 1, height - 1)
        geo.rotateX(-Math.PI / 2)
        const pos = geo.attributes['position'] as THREE.BufferAttribute

        for (let i = 0; i < pos.count; i++) {
          const col     = i % width
          const row     = Math.floor(i / width)
          const northing = (height - 1) - row
          const heightIn = data[col * width + northing]
          pos.setY(i, heightIn / 12.0)
        }
        pos.needsUpdate = true
        geo.computeVertexNormals()

        if (!cancelled) setGeometry(geo)
      } catch {
        // Non-fatal: background tiles are a visual aid, don't block the viewer
      }
    }
    load()
    return () => { cancelled = true }
  }, [binId, tileId])

  // Load ortho texture
  useEffect(() => {
    let cancelled = false
    const loader = new THREE.TextureLoader()
    loader.load(
      `/api/tileview/terrain/orthoImage/${tileId}`,
      tex => { if (!cancelled) setTexture(tex) },
      undefined,
      () => {},
    )
    return () => { cancelled = true }
  }, [tileId])

  if (!geometry) return null

  // Position tile in the rooftop viewer's local coordinate system.
  // PlaneGeometry is centered at origin, so after rotateX(-PI/2) the geometry
  // extends ±250 in X (easting) and ±250 in Z (-northing).  The SW corner of
  // the tile (min easting, min northing → min X, max Z) sits at (-250, 0, +250)
  // relative to the mesh origin, so we shift by half the tile size to place the
  // center correctly.
  const halfTile = 250  // half of the 500-ft tile side
  const xPos =  swNys[0] - buildingOffset.x_sw + halfTile
  const zPos = -(swNys[1] - buildingOffset.y_sw) - halfTile

  const material = texture
    ? new THREE.MeshStandardMaterial({ map: texture, roughness: 0.85, metalness: 0 })
    : new THREE.MeshStandardMaterial({ color: 0x888888, roughness: 0.85, metalness: 0 })

  return (
    <mesh
      geometry={geometry}
      material={material}
      position={[xPos, 0, zPos]}
    />
  )
}
