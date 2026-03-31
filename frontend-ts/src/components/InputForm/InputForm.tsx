import { useState } from 'react'
import type { ChangeEvent, FormEvent, ReactNode } from 'react'
import BuildingLookupField from './BuildingLookupField'
import FarEndPicker from '../FarEndPicker/FarEndPicker'

export interface RooftopSubmitValues {
  bin_id: string
  building_label?: string
  far_end_nys?: [number, number, number]   // rooftop-picker path
  lat?: number; lon?: number; alt_m?: number  // coords path (dark pattern)
  frequency_ghz: number
  mast_offset_ft: number
  sample_spacing?: number
}

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
  lat: '40.815328384719656',
  lon: '-73.95031852433306',
  alt_m: '88.5',
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
      }
    }

    setSubmitting(true)
    try { await onSubmit(submitValues) }
    finally { setSubmitting(false) }
  }

  return (
    <div style={styles.container}>
      <div style={styles.card}>
        <h1 style={styles.title}>LOS Analyzer</h1>
        <p style={styles.subtitle}>Line-of-sight analysis for NYC rooftop antennas</p>

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
                  value={values.sample_spacing}
                  onChange={set('sample_spacing')}
                  disabled={submitting}
                  style={{ width: '100%', cursor: 'pointer', accentColor: '#388bfd' }}
                />
                <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 4 }}>
                  {[
                    { v: 6,  label: 'High precision', sub: 'Slow' },
                    { v: 15, label: 'Medium',         sub: ''     },
                    { v: 24, label: 'Low precision',  sub: 'Fast' },
                  ].map(({ v, label, sub }) => (
                    <div key={v} onClick={() => setValues(val => ({ ...val, sample_spacing: String(v) }))}
                      style={{
                        textAlign: v === 6 ? 'left' : v === 24 ? 'right' : 'center',
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
            </Advanced>
          </Section>

          {error && <p style={styles.error}>{error}</p>}

          <button type="submit" style={styles.submit} disabled={submitting}>
            {submitting ? 'Submitting…' : 'Analyze →'}
          </button>
        </form>
      </div>

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

const styles: Record<string, React.CSSProperties> = {
  container: {
    flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
    padding: 24, overflowY: 'auto',
  },
  card: {
    width: '100%', maxWidth: 560,
    background: 'rgba(22,27,34,0.9)', border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 10, padding: '32px 36px',
  },
  title:    { fontSize: 22, fontWeight: 600, color: '#e6edf3', marginBottom: 6, letterSpacing: '-0.01em' },
  subtitle: { fontSize: 13, color: '#484f58', fontFamily: 'monospace', marginBottom: 28 },
  form:     { display: 'flex', flexDirection: 'column' },
  row:      { display: 'flex', gap: 12 },
  input: {
    width: '100%', background: 'rgba(13,17,23,0.8)',
    border: '1px solid rgba(255,255,255,0.1)', borderRadius: 6,
    color: '#e6edf3', fontSize: 13, fontFamily: 'monospace',
    padding: '8px 10px', outline: 'none', boxSizing: 'border-box',
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
