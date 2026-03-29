/**
 * Split-view tile map:
 * - Left (60%): React-Leaflet map with Fresnel ellipse, LOS line, tile polygons,
 *   intersection visualization overlays
 * - Right (40%): Tile3DViewer panel (shown when a tile is clicked)
 *
 * Driven directly by AnalysisOverview — no job polling.
 */
import { useState, useCallback, useEffect, useRef } from 'react'
import { MapContainer, TileLayer, GeoJSON, ImageOverlay, useMap } from 'react-leaflet'
import L from 'leaflet'
import type { Layer, PathOptions, CircleMarkerOptions } from 'leaflet'
import Tile3DViewer from '../Tile3DViewer/Tile3DViewer'
import 'leaflet/dist/leaflet.css'
import type { GeoJsonObject, Feature, Geometry, GeoJsonProperties } from 'geojson'

export interface AnalysisTile {
  id: string
  bounds: [[number, number], [number, number]]  // [[lon_sw, lat_sw], [lon_ne, lat_ne]]
  intersection_detected: boolean
}

export interface AnalysisOverview {
  endpoints: [[number, number, number], [number, number, number]]  // [lon, lat, alt]
  tiles: AnalysisTile[]
  overhead_ellipse_poly: [number, number][]  // [lon, lat] pairs
}

interface TileMapProps {
  overview: AnalysisOverview
  analysisId: string
}

const LOS_STYLE: PathOptions      = { color: '#e63030', weight: 2, opacity: 0.9 }
const ELLIPSE_STYLE: PathOptions  = { color: '#ff8800', weight: 2, dashArray: '8 5', fillOpacity: 0.08, fillColor: '#ff8800' }
const TILE_STYLE_CLEAR: PathOptions = { color: '#3388ff', weight: 1, fillOpacity: 0.05, fillColor: '#3388ff' }
const TILE_STYLE_OBS: PathOptions   = { color: '#e63030', weight: 2, fillOpacity: 0.15, fillColor: '#e63030' }

// Build a GeoJSON FeatureCollection of tile polygons from overview tiles
function buildTilesGeoJson(tiles: AnalysisTile[]): GeoJsonObject {
  return {
    type: 'FeatureCollection',
    features: tiles.map(tile => {
      const [[lon_sw, lat_sw], [lon_ne, lat_ne]] = tile.bounds
      return {
        type: 'Feature',
        geometry: {
          type: 'Polygon',
          coordinates: [[
            [lon_sw, lat_sw],
            [lon_ne, lat_sw],
            [lon_ne, lat_ne],
            [lon_sw, lat_ne],
            [lon_sw, lat_sw],
          ]],
        },
        properties: { id: tile.id, intersection_detected: tile.intersection_detected },
      }
    }),
  } as GeoJsonObject
}

// Build LOS line + endpoint markers as a GeoJSON FeatureCollection
function buildLosGeoJson(endpoints: [[number, number, number], [number, number, number]]): GeoJsonObject {
  const [[lon_a, lat_a], [lon_b, lat_b]] = endpoints
  return {
    type: 'FeatureCollection',
    features: [
      {
        type: 'Feature',
        geometry: { type: 'LineString', coordinates: [[lon_a, lat_a], [lon_b, lat_b]] },
        properties: {},
      },
      {
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [lon_a, lat_a] },
        properties: { label: 'A' },
      },
      {
        type: 'Feature',
        geometry: { type: 'Point', coordinates: [lon_b, lat_b] },
        properties: { label: 'B' },
      },
    ],
  } as GeoJsonObject
}

// Build Fresnel ellipse as a GeoJSON Polygon
function buildEllipseGeoJson(poly: [number, number][]): GeoJsonObject {
  return {
    type: 'Feature',
    geometry: { type: 'Polygon', coordinates: [poly as [number, number][]] },
    properties: {},
  } as GeoJsonObject
}

// Auto-fit map to endpoints on mount
function FitBounds({ endpoints }: { endpoints: [[number, number, number], [number, number, number]] }) {
  const map = useMap()
  useEffect(() => {
    const [[lon_a, lat_a], [lon_b, lat_b]] = endpoints
    const bounds: [[number, number], [number, number]] = [
      [Math.min(lat_a, lat_b) - 0.002, Math.min(lon_a, lon_b) - 0.002],
      [Math.max(lat_a, lat_b) + 0.002, Math.max(lon_a, lon_b) + 0.002],
    ]
    map.fitBounds(bounds, { padding: [30, 30] })
  }, [endpoints, map])
  return null
}

// Expose map instance to parent via ref
function MapRef({ mapRef }: { mapRef: React.MutableRefObject<L.Map | null> }) {
  const map = useMap()
  useEffect(() => { mapRef.current = map }, [map, mapRef])
  return null
}

// Invalidate Leaflet map size when the 3D panel opens/closes
function MapResizer({ panelOpen }: { panelOpen: boolean }) {
  const map = useMap()
  useEffect(() => {
    setTimeout(() => map.invalidateSize(), 60)
  }, [panelOpen, map])
  return null
}

export default function TileMap({ overview, analysisId }: TileMapProps) {
  const [activeTileId, setActiveTileId] = useState<string | null>(null)
  const [panelOpen, setPanelOpen]       = useState(false)
  const mapRef = useRef<L.Map | null>(null)

  const tilesGeoJson   = buildTilesGeoJson(overview.tiles)
  const losGeoJson     = buildLosGeoJson(overview.endpoints)
  const ellipseGeoJson = buildEllipseGeoJson(overview.overhead_ellipse_poly)

  const handleTileClick = useCallback((tileId: string) => {
    setActiveTileId(tileId)
    setPanelOpen(true)
    const tile = overview.tiles.find(t => t.id === tileId)
    if (tile) {
      const [[lon_sw, lat_sw], [lon_ne, lat_ne]] = tile.bounds
      // Delay slightly so the map has time to resize to 60% before fitting
      setTimeout(() => mapRef.current?.fitBounds([[lat_sw, lon_sw], [lat_ne, lon_ne]], { padding: [40, 40] }), 80)
    }
  }, [overview.tiles])

  const tileStyle = useCallback((feature: Feature<Geometry, GeoJsonProperties> | undefined): PathOptions =>
    feature?.properties?.['intersection_detected'] ? TILE_STYLE_OBS : TILE_STYLE_CLEAR
  , [])

  const onEachTile = useCallback((feature: Feature<Geometry, GeoJsonProperties>, layer: Layer) => {
    const p = feature.properties ?? {}
    layer.on('click', () => handleTileClick(p['id'] as string))
  }, [handleTileClick])

  // Approximate initial center; FitBounds will correct it
  const center: [number, number] = (() => {
    const [[lon_a, lat_a], [lon_b, lat_b]] = overview.endpoints
    return [(lat_a + lat_b) / 2, (lon_a + lon_b) / 2]
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
          <MapRef mapRef={mapRef} />
          <MapResizer panelOpen={panelOpen} />
          <FitBounds endpoints={overview.endpoints} />
          <TileLayer
            url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
            attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>'
            maxNativeZoom={19}
            maxZoom={22}
          />

          {/* Intersection visualization overlays — one per tile where intersection detected */}
          {overview.tiles.filter(t => t.intersection_detected).map(tile => {
            const [[lon_sw, lat_sw], [lon_ne, lat_ne]] = tile.bounds
            return (
              <ImageOverlay
                key={`overlay-${tile.id}`}
                url={`/api/analysis/intersectionVisualization/${analysisId}/${tile.id}`}
                bounds={[[lat_sw, lon_sw], [lat_ne, lon_ne]]}
                opacity={0.85}
                zIndex={200}
              />
            )
          })}

          {/* Fresnel ellipse */}
          {overview.overhead_ellipse_poly.length > 0 && (
            <GeoJSON key="ellipse" data={ellipseGeoJson} style={ELLIPSE_STYLE} />
          )}

          {/* Tile polygons */}
          <GeoJSON
            key={`tiles-${analysisId}`}
            data={tilesGeoJson}
            style={tileStyle}
            onEachFeature={onEachTile}
          />

          {/* LOS line + endpoint markers */}
          <GeoJSON
            key="los"
            data={losGeoJson}
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
        </MapContainer>
      </div>

      {/* 3D tile panel */}
      {panelOpen && (
        <div style={styles.panel3d}>
          <div style={styles.panel3dHeader}>
            <span style={styles.panel3dTitle}>{activeTileId || 'Tile 3D'}</span>
            <button
              style={styles.closeBtn}
              onClick={() => { setPanelOpen(false); setActiveTileId(null) }}
            >
              ×
            </button>
          </div>
          <div style={{ flex: 1, overflow: 'hidden' }}>
            <Tile3DViewer tileId={activeTileId} analysisId={analysisId} />
          </div>
        </div>
      )}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
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
