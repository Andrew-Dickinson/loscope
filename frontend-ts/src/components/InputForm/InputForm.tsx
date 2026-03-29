import { useState } from 'react'
import type { ChangeEvent, FormEvent } from 'react'
import type { ReactNode } from 'react'

export interface RooftopSubmitValues {
  bin_id: string
  lat: number
  lon: number
  alt_m: number
  frequency_ghz: number
  mast_offset_ft: number
  sample_spacing: number
}

interface InputFormProps {
  onSubmit: (values: RooftopSubmitValues) => Promise<void>
}

interface FormFieldValues {
  bin_id: string
  lat: string
  lon: string
  alt_m: string
  frequency_ghz: string
  mast_offset_ft: string
  sample_spacing: string
}

const DEFAULT_VALUES: FormFieldValues = {
  bin_id: '1058335',
  lat: '40.815328384719656',
  lon: '-73.95031852433306',
  alt_m: '88.5',
  frequency_ghz: '5',
  mast_offset_ft: '4',
  sample_spacing: '9',
}

export default function InputForm({ onSubmit }: InputFormProps) {
  const [values, setValues] = useState<FormFieldValues>(DEFAULT_VALUES)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState('')

  const set = (field: keyof FormFieldValues) => (e: ChangeEvent<HTMLInputElement>) =>
    setValues(v => ({ ...v, [field]: e.target.value }))

  const handleSubmit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')
    const { bin_id, lat, lon, alt_m, frequency_ghz, mast_offset_ft, sample_spacing } = values
    if (!bin_id.trim()) { setError('BIN is required'); return }
    const latF = parseFloat(lat), lonF = parseFloat(lon), altF = parseFloat(alt_m)
    if (isNaN(latF) || isNaN(lonF) || isNaN(altF)) { setError('Antenna coordinates are required'); return }

    setSubmitting(true)
    try {
      await onSubmit({
        bin_id: bin_id.trim(),
        lat: latF,
        lon: lonF,
        alt_m: altF,
        frequency_ghz: parseFloat(frequency_ghz) || 24,
        mast_offset_ft: parseFloat(mast_offset_ft) || 0,
        sample_spacing: parseInt(sample_spacing) || 5,
      })
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div style={styles.container}>
      <div style={styles.card}>
        <h1 style={styles.title}>LOS Analyzer</h1>
        <p style={styles.subtitle}>Line-of-sight analysis for NYC rooftop antennas</p>

        <form onSubmit={handleSubmit} style={styles.form}>
          <Section label="Building">
            <Field label="BIN" hint="Building Identification Number">
              <input style={styles.input} placeholder="e.g. 1058335"
                value={values.bin_id} onChange={set('bin_id')} disabled={submitting} />
            </Field>
          </Section>

          <Section label="Far-end Antenna">
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
          </Section>

          <Section label="Link Parameters">
            <div style={styles.row}>
              <Field label="Frequency (GHz)">
                <input style={styles.input} type="number" step="any" min="0.1" max="100"
                  value={values.frequency_ghz} onChange={set('frequency_ghz')} disabled={submitting} />
              </Field>
              <Field label="Mast offset (ft)">
                <input style={styles.input} type="number" step="any" min="0"
                  value={values.mast_offset_ft} onChange={set('mast_offset_ft')} disabled={submitting} />
              </Field>
              <Field label="Sample spacing (ft)">
                <input style={styles.input} type="number" step="1" min="1" max="50"
                  value={values.sample_spacing} onChange={set('sample_spacing')} disabled={submitting} />
              </Field>
            </div>
          </Section>

          {error && <p style={styles.error}>{error}</p>}

          <button type="submit" style={styles.submit} disabled={submitting}>
            {submitting ? 'Submitting…' : 'Analyze →'}
          </button>
        </form>
      </div>
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

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div style={{ flex: 1, minWidth: 0 }}>
      <label style={{
        display: 'block', marginBottom: 5,
        fontSize: 12, color: '#8b949e', fontFamily: 'monospace',
      }}>
        {label}
        {hint && <span style={{ color: '#3d444d', marginLeft: 6, fontSize: 11 }}>{hint}</span>}
      </label>
      {children}
    </div>
  )
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    flex: 1,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 24,
    overflowY: 'auto',
  },
  card: {
    width: '100%',
    maxWidth: 560,
    background: 'rgba(22,27,34,0.9)',
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 10,
    padding: '32px 36px',
  },
  title: {
    fontSize: 22,
    fontWeight: 600,
    color: '#e6edf3',
    marginBottom: 6,
    letterSpacing: '-0.01em',
  },
  subtitle: {
    fontSize: 13,
    color: '#484f58',
    fontFamily: 'monospace',
    marginBottom: 28,
  },
  form: {
    display: 'flex',
    flexDirection: 'column',
  },
  row: {
    display: 'flex',
    gap: 12,
  },
  input: {
    width: '100%',
    background: 'rgba(13,17,23,0.8)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 6,
    color: '#e6edf3',
    fontSize: 13,
    fontFamily: 'monospace',
    padding: '8px 10px',
    outline: 'none',
    transition: 'border-color 0.15s',
  },
  error: {
    color: '#ff4444',
    fontSize: 12,
    fontFamily: 'monospace',
    marginBottom: 12,
  },
  submit: {
    marginTop: 4,
    padding: '10px 0',
    borderRadius: 6,
    background: 'linear-gradient(135deg, #1f6feb, #388bfd)',
    color: '#fff',
    fontSize: 14,
    fontFamily: 'monospace',
    fontWeight: 600,
    cursor: 'pointer',
    border: 'none',
    letterSpacing: '0.03em',
    transition: 'opacity 0.15s',
  },
}
