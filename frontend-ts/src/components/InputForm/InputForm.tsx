import { useState } from 'react'
import type { ChangeEvent, FormEvent, ReactNode } from 'react'
import BuildingLookupField from './BuildingLookupField'
import FarEndPicker from '../FarEndPicker/FarEndPicker'
import logo from '../../assets/logo.svg'

export interface RooftopSubmitValues {
  bin_id: string
  building_label?: string
  far_end_nys?: [number, number, number]   // rooftop-picker path
  lat?: number; lon?: number; alt_m?: number  // coords path (dark pattern)
  frequency_ghz: number
  mast_offset_ft: number
  sample_spacing?: number
  obstruction_types: string[]
}

// Must match backend ObstructionType's snake_case serialization (backend-rs/src/types/obstructions.rs).
const OBSTRUCTION_TYPES: { value: string; label: string }[] = [
  { value: 'active_permits',              label: 'Active Building Permits' },
  { value: 'approved_job_applications',   label: 'Approved Building Permit Applications' },
  { value: 'recent_job_applications',     label: 'Recent Building Permit Applications' },
  { value: 'new_construction_co',         label: 'New Certificates of Occupancy' },
  { value: 'new_construction_footprints', label: 'Building Footprints Marked "New Construction"' },
  { value: 'non_surveyed_bridge',         label: 'Major Bridges (missing from Lidar)' },
]

interface InputFormProps {
  onSubmit: (values: RooftopSubmitValues) => Promise<void>
}

interface FormFieldValues {
  lat: string
  lon: string
  alt_m: string
  frequency_ghz: string
  mast_offset_ft: string
  sample_spacing: string
}

const DEFAULT_VALUES: FormFieldValues = {
  lat: '',
  lon: '',
  alt_m: '',
  frequency_ghz: '5',
  mast_offset_ft: '4',
  sample_spacing: '15',
}

export default function InputForm({ onSubmit }: InputFormProps) {
  const [values, setValues]     = useState<FormFieldValues>(DEFAULT_VALUES)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError]       = useState('')

  // Building
  const [buildingBin,   setBuildingBin]   = useState<string | null>(null)
  const [buildingLabel, setBuildingLabel] = useState<string | null>(null)

  // Far-end: main path = rooftop picker
  const [farEndMode,   setFarEndMode]   = useState<'rooftop' | 'coords'>('rooftop')
  const [farEndBin,    setFarEndBin]    = useState<string | null>(null)
  const [farEndLabel,  setFarEndLabel]  = useState('')
  const [farEndNys,    setFarEndNys]    = useState<[number, number, number] | null>(null)
  const [showPicker,   setShowPicker]   = useState(false)
  const [manualSampling, setManualSampling] = useState(false)
  const [obstructionTypes, setObstructionTypes] = useState<Record<string, boolean>>(
    () => Object.fromEntries(OBSTRUCTION_TYPES.map(t => [t.value, true])),
  )
  const enabledObstructionTypes = OBSTRUCTION_TYPES.filter(t => obstructionTypes[t.value]).map(t => t.value)

  const set = (field: keyof FormFieldValues) => (e: ChangeEvent<HTMLInputElement>) =>
    setValues(v => ({ ...v, [field]: e.target.value }))

  const handleSubmit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')

    if (!buildingBin) { setError('Select a building first'); return }

    let submitValues: RooftopSubmitValues

    if (farEndMode === 'rooftop') {
      if (!farEndBin)  { setError('Select a far-end building'); return }
      if (!farEndNys)  { setError('Place the far-end antenna on the rooftop'); return }
      submitValues = {
        bin_id: buildingBin,
        building_label: buildingLabel ?? undefined,
        far_end_nys: farEndNys,
        frequency_ghz: parseFloat(values.frequency_ghz) || 24,
        mast_offset_ft: parseFloat(values.mast_offset_ft) || 0,
        sample_spacing: manualSampling ? undefined : (parseInt(values.sample_spacing) || 15),
        obstruction_types: enabledObstructionTypes,
      }
    } else {
      const latF = parseFloat(values.lat), lonF = parseFloat(values.lon), altF = parseFloat(values.alt_m)
      if (isNaN(latF) || isNaN(lonF) || isNaN(altF)) { setError('Antenna coordinates are required'); return }
      submitValues = {
        bin_id: buildingBin,
        building_label: buildingLabel ?? undefined,
        lat: latF, lon: lonF, alt_m: altF,
        frequency_ghz: parseFloat(values.frequency_ghz) || 24,
        mast_offset_ft: parseFloat(values.mast_offset_ft) || 0,
        sample_spacing: manualSampling ? undefined : (parseInt(values.sample_spacing) || 15),
        obstruction_types: enabledObstructionTypes,
      }
    }

    setSubmitting(true)
    try { await onSubmit(submitValues) }
    finally { setSubmitting(false) }
  }

  return (
    <div style={styles.container}>
      <div style={styles.contentWrap}>
      <div style={styles.card}>
        <img src={logo} alt="LOScope" style={styles.logo} />

        <form onSubmit={handleSubmit} style={styles.form}>

          <Section label="Building">
            <BuildingLookupField
              disabled={submitting}
              onBinResolved={(bin, label) => { setBuildingBin(bin); setBuildingLabel(label) }}
              onBinCleared={() => { setBuildingBin(null); setBuildingLabel(null) }}
            />
          </Section>

          <Section label="Far-end Antenna">
            {farEndMode === 'rooftop' ? (
              <>
                <BuildingLookupField
                  disabled={submitting}
                  onBinResolved={(bin, label) => {
                    setFarEndBin(bin)
                    setFarEndLabel(label)
                    setFarEndNys(null)
                  }}
                  onBinCleared={() => {
                    setFarEndBin(null)
                    setFarEndLabel('')
                    setFarEndNys(null)
                  }}
                />

                {farEndBin && !farEndNys && (
                  <button type="button" style={styles.pickBtn} disabled={submitting}
                    onClick={() => setShowPicker(true)}>
                    Pick on rooftop →
                  </button>
                )}

                {farEndNys && (
                  <div style={styles.placedBadge}>
                    Antenna placed on
                    <span style={styles.placedLabel}> {farEndLabel}</span>
                    <button type="button" style={styles.clearBtn}
                      onClick={() => setFarEndNys(null)} disabled={submitting}>
                      ×
                    </button>
                  </div>
                )}

                <div style={styles.darkPatternRow}>
                  <button type="button" style={styles.darkPatternLink} disabled={submitting}
                    onClick={() => { setFarEndMode('coords'); setFarEndBin(null); setFarEndNys(null) }}>
                    Enter coordinates manually
                  </button>
                </div>
              </>
            ) : (
              <>
                <div style={styles.row}>
                  <Field label="Latitude">
                    <input style={styles.input} type="number" step="any" placeholder="40.8438"
                      value={values.lat} onChange={set('lat')} disabled={submitting} />
                  </Field>
                  <Field label="Longitude">
                    <input style={styles.input} type="number" step="any" placeholder="-73.8930"
                      value={values.lon} onChange={set('lon')} disabled={submitting} />
                  </Field>
                  <Field label="Altitude (m)">
                    <input style={styles.input} type="number" step="any" placeholder="69"
                      value={values.alt_m} onChange={set('alt_m')} disabled={submitting} />
                  </Field>
                </div>
                <div style={styles.darkPatternRow}>
                  <button type="button" style={styles.darkPatternLink} disabled={submitting}
                    onClick={() => setFarEndMode('rooftop')}>
                    Use rooftop picker instead
                  </button>
                </div>
              </>
            )}
          </Section>

          <Section label="Link Parameters">
            {!manualSampling && (
              <div style={{ marginBottom: 2 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 5 }}>
                  <span style={{ fontSize: 12, color: '#8b949e', fontFamily: 'monospace' }}>Auto-sample spacing (ft)</span>
                  <span style={{ fontSize: 12, fontFamily: 'monospace', color: '#8b949e' }}>{values.sample_spacing}</span>
                </div>
                <input
                  type="range" min={6} max={24} step={3}
                  value={30 - parseInt(values.sample_spacing)}
                  onChange={e => setValues(val => ({ ...val, sample_spacing: String(30 - parseInt(e.target.value)) }))}
                  disabled={submitting}
                  style={{ width: '100%', cursor: 'pointer', accentColor: '#388bfd' }}
                />
                <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 4 }}>
                  {[
                    { v: 24, label: 'Low precision', sub: 'Fast' },
                    { v: 15, label: 'Medium',        sub: ''     },
                    { v: 6,  label: 'High precision', sub: 'Slow' },
                  ].map(({ v, label, sub }) => (
                    <div key={v} onClick={() => setValues(val => ({ ...val, sample_spacing: String(v) }))}
                      style={{
                        textAlign: v === 24 ? 'left' : v === 6 ? 'right' : 'center',
                        fontSize: 11, fontFamily: 'monospace',
                        color: parseInt(values.sample_spacing) === v ? '#388bfd' : '#5a6472',
                        transition: 'color 0.15s',
                        lineHeight: 1.4,
                        cursor: 'pointer',
                      }}>
                      {label}{sub && <><br />{sub}</>}
                    </div>
                  ))}
                </div>
              </div>
            )}
            <div style={styles.darkPatternRow}>
              {manualSampling ? (
                <button type="button" style={styles.darkPatternLink} disabled={submitting}
                  onClick={() => setManualSampling(false)}>Use automatic sampling</button>
              ) : (
                <button type="button" style={styles.darkPatternLink} disabled={submitting}
                  onClick={() => setManualSampling(true)}>Switch to Manual Sampling</button>
              )}
            </div>
            <Advanced>
              <div style={styles.row}>
                <Field label="Frequency (GHz)">
                  <input style={styles.input} type="number" step="any" min="0.1" max="100"
                    value={values.frequency_ghz} onChange={set('frequency_ghz')} disabled={submitting} />
                </Field>
                <Field label="Mast offset (ft)">
                  <input style={styles.input} type="number" step="any" min="0"
                    value={values.mast_offset_ft} onChange={set('mast_offset_ft')} disabled={submitting} />
                </Field>
              </div>

              <div style={{ marginTop: 14 }}>
                <label style={{ display: 'block', marginBottom: 6, fontSize: 12, color: '#8b949e', fontFamily: 'monospace' }}>
                  Enabled Obstruction types
                </label>
                <div style={styles.checkboxGrid}>
                  {OBSTRUCTION_TYPES.map(t => (
                    <label key={t.value} style={styles.checkboxLabel}>
                      <input
                        type="checkbox"
                        checked={obstructionTypes[t.value] ?? false}
                        onChange={e => setObstructionTypes(prev => ({ ...prev, [t.value]: e.target.checked }))}
                        disabled={submitting}
                        style={styles.checkbox}
                      />
                      {t.label}
                    </label>
                  ))}
                </div>
              </div>
            </Advanced>
          </Section>

          {error && <p style={styles.error}>{error}</p>}

          <button type="submit" style={styles.submit} disabled={submitting}>
            {submitting ? 'Submitting…' : 'Analyze →'}
          </button>
        </form>
      </div>
      </div>

      <Footer />

      {showPicker && farEndBin && (
        <FarEndPicker
          binId={farEndBin}
          label={farEndLabel}
          onConfirm={(nys_e, nys_n, nys_z) => {
            setFarEndNys([nys_e, nys_n, nys_z])
            setShowPicker(false)
          }}
          onCancel={() => setShowPicker(false)}
        />
      )}
    </div>
  )
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div style={{ marginBottom: 24 }}>
      <div style={{
        fontSize: 10, fontFamily: 'monospace', color: '#484f58',
        textTransform: 'uppercase', letterSpacing: '0.08em',
        marginBottom: 10, paddingBottom: 6,
        borderBottom: '1px solid rgba(255,255,255,0.06)',
      }}>{label}</div>
      {children}
    </div>
  )
}

function Advanced({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div style={{ marginTop: 10 }}>
      <button type="button" onClick={() => setOpen(o => !o)} style={{
        background: 'none', border: 'none', padding: 0, cursor: 'pointer',
        fontSize: 11, fontFamily: 'monospace', color: '#5a6472',
        letterSpacing: '0.02em', display: 'flex', alignItems: 'center', gap: 4,
      }}>
        <span style={{ display: 'inline-block', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 0.15s' }}>›</span>
        Advanced
      </button>
      {open && <div style={{ marginTop: 10 }}>{children}</div>}
    </div>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div style={{ flex: 1, minWidth: 0 }}>
      <label style={{ display: 'block', marginBottom: 5, fontSize: 12, color: '#8b949e', fontFamily: 'monospace' }}>
        {label}
      </label>
      {children}
    </div>
  )
}

const OTHER_PROJECTS: { label: string; href: string }[] = [
  { label: 'IP Explorer',     href: 'http://ip-explorer.andrew.mesh.nycmesh.net/' },
  { label: 'Node Explorer',   href: 'https://node-explorer.andrew.mesh.nycmesh.net/' },
  { label: 'Outage Simulator', href: 'https://node-explorer.andrew.mesh.nycmesh.net/outage-analyzer' },
  { label: 'OSPF API v1',     href: 'https://api.andrew.mesh.nycmesh.net/api/v1/mesh_ospf_data.json' },
  { label: 'OSPF API v2',     href: 'https://api.andrew.mesh.nycmesh.net/api/v2/mesh_ospf_data.json' },
  { label: 'OSPF Event Feed', href: 'https://api.andrew.mesh.nycmesh.net/api/v2/ospf-event-stream/viewer.html' },
]

const REPO_URL = 'https://github.com/Andrew-Dickinson/loscope'
// Set via --build-arg GIT_COMMIT_HASH in Dockerfile.frontend (see docker_build.sh); undefined
// in local `npm run dev`, where there's no single meaningful commit to point at.
const COMMIT_SHA: string | undefined = import.meta.env.VITE_GIT_COMMIT_HASH || undefined
const COMMIT_SHORT = COMMIT_SHA?.slice(0, 7)

function Footer() {
  return (
    <footer className="los-footer" style={styles.footer}>
      <style>{`
        .los-footer a { color: inherit; text-decoration: none; transition: color 0.15s; }
        .los-footer a:hover { color: #e6edf3; }
        @media (min-width: 900px) {
          .los-footer-inner { flex-direction: row !important; }
        }
      `}</style>
      <div className="los-footer-inner" style={styles.footerInner}>
        <div style={styles.footerLinks}>
          <span style={{ color: '#6e7681' }}>Other projects:</span>
          {OTHER_PROJECTS.map(p => (
            <a key={p.href} href={p.href} target="_blank" rel="noopener noreferrer" style={{ color: '#8b949e' }}>
              {p.label}
            </a>
          ))}
        </div>
        <div style={styles.footerSource}>
          <a
            href={REPO_URL}
            target="_blank"
            rel="noopener noreferrer"
            style={{ display: 'flex', alignItems: 'center', gap: 6, color: '#8b949e' }}
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" fill="currentColor" viewBox="0 0 24 24">
              <title>GitHub</title>
              <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
            </svg>
            <span>Source Code</span>
          </a>
          {COMMIT_SHA && (
            <span style={{ fontSize: 11, color: '#484f58' }}>
              (<a href={`${REPO_URL}/commit/${COMMIT_SHA}`} target="_blank" rel="noopener noreferrer" style={{ color: '#6e7681' }}>{COMMIT_SHORT}</a>)
            </span>
          )}
        </div>
      </div>
    </footer>
  )
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    flex: 1, display: 'flex', flexDirection: 'column',
    padding: '24px 24px 0', overflowY: 'auto',
  },
  // Wraps just the card, centering it as a group on wide screens: grows to fill leftover
  // vertical space (centering the card via its own margin:auto below) but never shrinks below
  // the card's natural height — shrinking here caused the card's overflowing content to
  // visually overlap the footer instead of pushing it down.
  contentWrap: {
    flex: '1 0 auto',
    width: '100%', maxWidth: 1005,
    margin: '0 auto',
    display: 'flex',
  },
  card: {
    width: '100%', maxWidth: 560,
    margin: 'auto',
    background: 'rgba(22,27,34,0.9)', border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 10, padding: '32px 36px',
  },
  // Full-width strip so its top border spans the whole screen; footerInner (below) re-applies
  // the same max-width as contentWrap so the link content still aligns with the card above.
  footer: {
    width: '100%',
    flexShrink: 0,
    marginTop: 24,
    borderTop: '1px solid rgba(255,255,255,0.06)',
    background: '#0d1117',
  },
  footerInner: {
    width: '100%', maxWidth: 1005,
    margin: '0 auto',
    display: 'flex', flexDirection: 'column', gap: 12,
    alignItems: 'center', justifyContent: 'space-between',
    padding: '16px 24px 24px',
    fontSize: 12, fontFamily: 'monospace',
  },
  footerLinks: {
    display: 'flex', flexWrap: 'wrap', gap: 12,
    justifyContent: 'center',
  },
  footerSource: {
    display: 'flex', alignItems: 'center', gap: 8,
  },
  logo:     { height: 64, marginBottom: 28, display: 'block' },
  form:     { display: 'flex', flexDirection: 'column' },
  row:      { display: 'flex', gap: 12 },
  input: {
    width: '100%', background: 'rgba(13,17,23,0.8)',
    border: '1px solid rgba(255,255,255,0.1)', borderRadius: 6,
    color: '#e6edf3', fontSize: 13, fontFamily: 'monospace',
    padding: '8px 10px', outline: 'none', boxSizing: 'border-box',
  },
  checkboxGrid: {
    display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '6px 12px',
  },
  checkboxLabel: {
    display: 'flex', alignItems: 'center', gap: 6,
    fontSize: 12, color: '#8b949e', fontFamily: 'monospace', cursor: 'pointer',
  },
  checkbox: {
    accentColor: '#388bfd', cursor: 'pointer', margin: 0,
  },
  pickBtn: {
    marginTop: 10, width: '100%', padding: '8px 0', borderRadius: 6,
    background: 'rgba(77,159,255,0.08)', border: '1px solid rgba(77,159,255,0.3)',
    color: '#4d9fff', fontSize: 13, fontFamily: 'monospace',
    cursor: 'pointer', letterSpacing: '0.02em',
  },
  placedBadge: {
    marginTop: 8, fontSize: 11, fontFamily: 'monospace', color: '#484f58',
    display: 'flex', alignItems: 'center', gap: 0,
  },
  placedLabel: { color: '#388bfd', marginLeft: 4 },
  clearBtn: {
    background: 'none', border: 'none', color: '#484f58',
    cursor: 'pointer', fontSize: 14, padding: '0 0 0 6px', lineHeight: 1,
  },
  darkPatternRow: { marginTop: 8, display: 'flex', justifyContent: 'flex-end' },
  darkPatternLink: {
    background: 'none', border: 'none', padding: 0, cursor: 'pointer',
    fontSize: 11, fontFamily: 'monospace', color: '#5a6472', letterSpacing: '0.02em',
  },
  error:  { color: '#ff4444', fontSize: 12, fontFamily: 'monospace', marginBottom: 12 },
  submit: {
    marginTop: 4, padding: '10px 0', borderRadius: 6,
    background: 'linear-gradient(135deg, #1f6feb, #388bfd)',
    color: '#fff', fontSize: 14, fontFamily: 'monospace',
    fontWeight: 600, cursor: 'pointer', border: 'none', letterSpacing: '0.03em',
  },
}
