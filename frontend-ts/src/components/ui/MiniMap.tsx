// Interactive OSM mini-map for the lower-right corner of the rooftop viewer, showing the
// footprint of the currently displayed building in the context of the surrounding city.
// Fetches /api/rooftop/footprintGeoJson/<binId> (a GeoJSON Feature, WGS84 lon/lat).
import { useState, useEffect } from 'react'
import { MapContainer, TileLayer, GeoJSON, CircleMarker, useMapEvents } from 'react-leaflet'
import type { PathOptions } from 'leaflet'
import 'leaflet/dist/leaflet.css'
import { fetchWithRetry } from '../../lib/fetchWithRetry'

interface FootprintFeature {
  type: 'Feature'
  geometry: { type: 'Polygon'; coordinates: number[][][] }
}

const SIZE = 204
// Fallback for the degenerate case of an empty/pointless ring — shouldn't happen in practice.
const DEFAULT_ZOOM = 17
// The dot breakpoint is expressed as an offset below the fitted default zoom (rather than an
// absolute zoom level) so it scales along with the per-footprint fit: a large footprint's
// default zoom is already lower, and the dot still kicks in that same number of levels below it.
const DOT_ZOOM_OFFSET = 2
const FOOTPRINT_STYLE: PathOptions = { color: '#f85149', weight: 2, fillOpacity: 0.25, fillColor: '#f85149' }

const EARTH_METERS_PER_DEG_LAT = 111320
// Standard Web Mercator ground resolution constant (meters/pixel at zoom 0, latitude 0).
const WEB_MERCATOR_C = 156543.03392804097

function bounds(rings: number[][][]) {
  const points = rings[0] ?? []
  const lons = points.map(p => p[0])
  const lats = points.map(p => p[1])
  return {
    minLon: Math.min(...lons), maxLon: Math.max(...lons),
    minLat: Math.min(...lats), maxLat: Math.max(...lats),
  }
}

function centroid(rings: number[][][]): [number, number] {
  const { minLon, maxLon, minLat, maxLat } = bounds(rings)
  return [(minLat + maxLat) / 2, (minLon + maxLon) / 2]
}

// Picks the smallest zoom at which the footprint's longer side is at least half the mini-map's
// pixel size, so tiny buildings don't shrink to an unreadable speck and large ones don't get
// needlessly clipped. Rounds up (more zoomed in) so "at least half" always holds.
function fitZoomForFootprint(rings: number[][][], viewportPx: number): number {
  const { minLon, maxLon, minLat, maxLat } = bounds(rings)
  if (!Number.isFinite(minLon) || !Number.isFinite(minLat)) return DEFAULT_ZOOM

  const latCenter = (minLat + maxLat) / 2
  const widthM = (maxLon - minLon) * EARTH_METERS_PER_DEG_LAT * Math.cos(latCenter * Math.PI / 180)
  const heightM = (maxLat - minLat) * EARTH_METERS_PER_DEG_LAT
  const spanM = Math.max(widthM, heightM, 3)  // floor avoids absurd zoom on a near-zero-size footprint

  const targetPx = viewportPx / 2
  const idealZoom = Math.log2((targetPx * WEB_MERCATOR_C * Math.cos(latCenter * Math.PI / 180)) / spanM)
  return Math.min(19, Math.max(10, Math.ceil(idealZoom)))
}

function FootprintOverlay({ feature, center, initialZoom }: { feature: FootprintFeature; center: [number, number]; initialZoom: number }) {
  const [zoom, setZoom] = useState(initialZoom)
  useMapEvents({ zoomend: e => setZoom(e.target.getZoom()) })

  if (zoom < initialZoom - DOT_ZOOM_OFFSET) {
    // CircleMarker's radius is in screen pixels, not meters, so it stays visible at any zoom.
    return <CircleMarker center={center} radius={11.25} pathOptions={FOOTPRINT_STYLE} />
  }
  return <GeoJSON data={feature} style={FOOTPRINT_STYLE} />
}

export default function MiniMap({ binId, className }: { binId: string; className?: string }) {
  const [feature, setFeature] = useState<FootprintFeature | null>(null)

  useEffect(() => {
    let cancelled = false
    setFeature(null)
    fetchWithRetry(`/api/rooftop/footprintGeoJson/${binId}`, undefined, () => cancelled)
      .then(r => r.json())
      .then((f: FootprintFeature) => { if (!cancelled) setFeature(f) })
      .catch(() => {})
    return () => { cancelled = true }
  }, [binId])

  if (!feature) return null

  const center = centroid(feature.geometry.coordinates)
  const zoom = fitZoomForFootprint(feature.geometry.coordinates, SIZE)

  return (
    <div className={className} style={styles.wrapper}>
      <div style={styles.mapBox}>
        {/* Keyed on binId: a plain remount is the simplest way to recenter when the
            displayed building changes, since react-leaflet only honors center/zoom on mount. */}
        <MapContainer key={binId} center={center} zoom={zoom} attributionControl={false} style={{ height: '100%', width: '100%' }}>
          <TileLayer
            url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
            maxNativeZoom={19}
            maxZoom={22}
          />
          <FootprintOverlay feature={feature} center={center} initialZoom={zoom} />
        </MapContainer>
      </div>
      <a
        href="https://www.openstreetmap.org/copyright"
        target="_blank"
        rel="noreferrer"
        style={styles.attribution}
      >
        © OpenStreetMap contributors
      </a>
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  wrapper: {
    position: 'absolute',
    right: 16,
    bottom: 16,
    width: SIZE,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'flex-end',
  },
  mapBox: {
    width: SIZE,
    height: SIZE,
    borderRadius: 8,
    overflow: 'hidden',
    border: '1px solid rgba(255, 255, 255, 0.14)',
    boxShadow: '0 1px 4px rgba(0, 0, 0, 0.4)',
  },
  attribution: {
    marginTop: 4,
    padding: '2px 6px',
    borderRadius: 4,
    background: 'rgba(13, 17, 23, 0.85)',
    border: '1px solid rgba(255, 255, 255, 0.14)',
    fontSize: 9,
    fontFamily: 'monospace',
    color: '#8b949e',
    textDecoration: 'none',
  },
}
