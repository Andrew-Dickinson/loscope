/**
 * Split-view tile map:
 * - Left (60%): React-Leaflet map with Fresnel ellipse, LOS line, tile overlays
 * - Right (40%): Tile3DViewer panel (shown when a tile is clicked)
 */
import { useState, useCallback, useEffect } from 'react'
import { MapContainer, TileLayer, GeoJSON, ImageOverlay, useMap } from 'react-leaflet'
import L from 'leaflet'
import type { Layer, PathOptions, CircleMarkerOptions } from 'leaflet'
import Tile3DViewer from '../Tile3DViewer/Tile3DViewer'
import 'leaflet/dist/leaflet.css'
import type { JobState } from '../../hooks/useJob'
import type { RooftopResult } from '../RooftopViewer/RooftopViewer'
import type { GeoJsonObject, Feature, Geometry, GeoJsonProperties } from 'geojson'

interface TileOverlay {
  url: string
  bounds: [[number, number], [number, number]]
}

interface TileMapResult {
  tiles: GeoJsonObject | null
  tile_overlays: TileOverlay[] | null
  fresnel_ellipse: GeoJsonObject | null
  los_line: {
    features: Feature<Geometry, GeoJsonProperties>[]
  } | null
  tile_heightmaps: Record<string, { url: string; min_height_in: number; max_height_in: number }> | null
  tile_ortho_textures: Record<string, string> | null
  obs_rasters: unknown
  tile_obs_info: unknown
  nys_a: [number, number, number]
  nys_b: [number, number, number]
  frequency_ghz: number
  job_id?: string
}

interface TileMapProps {
  tileMapJob: JobState | null
  rooftopResult: RooftopResult | null
  frequency_ghz: number
}

const LOS_STYLE: PathOptions  = { color: '#e63030', weight: 2, opacity: 0.9 }
const ELLIPSE_STYLE: PathOptions = {
  color: '#ff8800', weight: 2, dashArray: '8 5',
  fillOpacity: 0.08, fillColor: '#ff8800',
}
const TILE_STYLE_CLEAR: PathOptions = { color: '#3388ff', weight: 1, fillOpacity: 0.05, fillColor: '#3388ff' }
const TILE_STYLE_OBS: PathOptions   = { color: '#e63030', weight: 2, fillOpacity: 0.15, fillColor: '#e63030' }

// Helper to make a Leaflet LatLngBounds from GeoJSON features
function geoJsonBounds(features: Feature<Geometry, GeoJsonProperties>[]): [[number, number], [number, number]] | null {
  if (!features || features.length === 0) return null
  const lats: number[] = [], lons: number[] = []
  const collect = (coords: unknown): void => {
    if (Array.isArray(coords) && coords.length >= 2 && typeof coords[0] === 'number') {
      lons.push(coords[0] as number)
      lats.push(coords[1] as number)
      return
    }
    if (Array.isArray(coords)) coords.forEach(collect)
  }
  features.forEach(f => collect((f.geometry as { coordinates: unknown }).coordinates))
  if (lats.length === 0) return null
  return [
    [Math.min(...lats) - 0.002, Math.min(...lons) - 0.002],
    [Math.max(...lats) + 0.002, Math.max(...lons) + 0.002],
  ]
}

// Invalidate Leaflet map size when panel is resized
function MapResizer({ panelOpen }: { panelOpen: boolean }) {
  const map = useMap()
  useEffect(() => {
    setTimeout(() => map.invalidateSize(), 60)
  }, [panelOpen, map])
  return null
}

// Auto-fit map to LOS line bounds when result arrives
function FitBounds({ result }: { result: TileMapResult | null }) {
  const map = useMap()
  useEffect(() => {
    if (!result) return
    const bounds = geoJsonBounds(result.los_line?.features || [])
    if (bounds) map.fitBounds(bounds, { padding: [30, 30] })
  }, [result, map])
  return null
}

export default function TileMap({ tileMapJob, rooftopResult: _rooftopResult, frequency_ghz }: TileMapProps) {
  const [activeTileId, setActiveTileId]   = useState<string | null>(null)
  const [tile3dJobId,  setTile3dJobId]    = useState<string | null>(null)
  const [panelOpen,    setPanelOpen]      = useState(false)

  const result = tileMapJob?.status === 'done' ? (tileMapJob.result as unknown as TileMapResult) : null

  const handleTileClick = useCallback(async (tileId: string) => {
    if (!result) return
    setActiveTileId(tileId)
    setTile3dJobId(null)
    setPanelOpen(true)

    try {
      const res = await fetch('/api/tile-3d', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          tile_id: tileId,
          nys_a: result.nys_a,
          nys_b: result.nys_b,
          frequency_ghz,
        }),
      })
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const { job_id } = await res.json() as { job_id: string }
      setTile3dJobId(job_id)
    } catch (err) {
      console.error('Tile-3d submit failed:', err)
    }
  }, [result, frequency_ghz])

  const tileStyle = useCallback((feature: Feature<Geometry, GeoJsonProperties> | undefined): PathOptions =>
    feature?.properties?.['hasObstruction'] ? TILE_STYLE_OBS : TILE_STYLE_CLEAR
  , [])

  const onEachTile = useCallback((feature: Feature<Geometry, GeoJsonProperties>, layer: Layer) => {
    const p = feature.properties ?? {}
    layer.on('click', () => handleTileClick(p['id'] as string))
    let html = `<code>${p['id']}</code>`
    if (p['hasObstruction']) html += ' <span style="color:#e63030;font-weight:bold">obstructed</span>'
    if (p['obstructions']?.length) {
      html += `<br/><b>${(p['obstructions'] as unknown[]).length} obstruction(s)</b>`
    }
    if ('bindPopup' in layer) (layer as L.Path).bindPopup(html)
  }, [handleTileClick])

  // Approximate map center — FitBounds will auto-fit when result arrives
  const center: [number, number] = (() => {
    if (!result) return [40.843, -73.93]
    const pts = (result.los_line?.features || []).filter(f => f.geometry.type === 'Point')
    if (pts.length >= 2) {
      const coords0 = (pts[0].geometry as { coordinates: [number, number] }).coordinates
      const coords1 = (pts[1].geometry as { coordinates: [number, number] }).coordinates
      return [(coords0[1] + coords1[1]) / 2, (coords0[0] + coords1[0]) / 2]
    }
    return [40.843, -73.93]
  })()

  return (
    <div style={{ height: '100%', display: 'flex' }}>
      {/* Map panel */}
      <div style={{ flex: panelOpen ? '0 0 60%' : 1, height: '100%', position: 'relative' }}>
        <MapContainer
          center={center}
          zoom={14}
          maxZoom={22}
          style={{ height: '100%', width: '100%' }}
        >
          <MapResizer panelOpen={panelOpen} />
          <FitBounds result={result} />
          <TileLayer
            url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
            attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
            maxNativeZoom={19}
            maxZoom={22}
          />

          {result && (
            <>
              {/* Obstruction overlays */}
              {result.tile_overlays?.map((o, i) => (
                <ImageOverlay key={i} url={o.url} bounds={o.bounds} opacity={0.85} zIndex={200} />
              ))}

              {/* Fresnel ellipse */}
              {result.fresnel_ellipse && (
                <GeoJSON key="ellipse" data={result.fresnel_ellipse} style={ELLIPSE_STYLE} />
              )}

              {/* Tile polygons */}
              {result.tiles && (
                <GeoJSON
                  key={`tiles-${result.job_id}`}
                  data={result.tiles}
                  style={tileStyle}
                  onEachFeature={onEachTile}
                />
              )}

              {/* LOS line + point markers */}
              {result.los_line && (
                <GeoJSON
                  key="los"
                  data={result.los_line as unknown as GeoJsonObject}
                  style={LOS_STYLE}
                  pointToLayer={(feature: Feature<Geometry, GeoJsonProperties>, latlng: L.LatLng) => {
                    const label = feature.properties?.['label'] as string | undefined
                    const opts: CircleMarkerOptions = {
                      radius: 7, color: '#e63030',
                      fillColor: '#e63030', fillOpacity: 1, weight: 2,
                    }
                    const marker = L.circleMarker(latlng, opts)
                    if (label) marker.bindTooltip(label, { permanent: true, direction: 'right' })
                    return marker
                  }}
                />
              )}
            </>
          )}

          {!result && tileMapJob?.status !== 'error' && (
            <div style={styles.mapOverlay}>
              <span style={{ color: '#484f58', fontFamily: 'monospace', fontSize: 13 }}>
                {tileMapJob ? 'Computing…' : 'Loading…'}
              </span>
            </div>
          )}
        </MapContainer>
      </div>

      {/* 3D panel */}
      {panelOpen && (
        <div style={styles.panel3d}>
          <div style={styles.panel3dHeader}>
            <span style={styles.panel3dTitle}>{activeTileId || 'Tile 3D'}</span>
            <button
              style={styles.closeBtn}
              onClick={() => { setPanelOpen(false); setActiveTileId(null); setTile3dJobId(null) }}
            >
              ×
            </button>
          </div>
          <div style={{ flex: 1, overflow: 'hidden' }}>
            <Tile3DViewer
              jobId={tile3dJobId}
              tileId={activeTileId}
              tileHeightmap={result?.tile_heightmaps?.[activeTileId ?? ''] ?? null}
              tileOrtho={result?.tile_ortho_textures?.[activeTileId ?? ''] ?? null}
            />
          </div>
        </div>
      )}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  mapOverlay: {
    position: 'absolute', inset: 0, zIndex: 1000,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
    pointerEvents: 'none',
  },
  panel3d: {
    flex: '0 0 40%',
    height: '100%',
    display: 'flex',
    flexDirection: 'column',
    background: '#0e1117',
    borderLeft: '2px solid #1c2128',
  },
  panel3dHeader: {
    flexShrink: 0,
    height: 38,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '0 12px',
    background: '#161b22',
    borderBottom: '1px solid #1c2128',
  },
  panel3dTitle: {
    color: '#8b949e',
    fontSize: 12,
    fontFamily: 'monospace',
  },
  closeBtn: {
    color: '#484f58',
    fontSize: 20,
    lineHeight: 1,
    padding: '0 4px',
    cursor: 'pointer',
    background: 'none',
    border: 'none',
  },
}
