import { useState, useRef, useEffect } from 'react'
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

interface GeoFeature {
  properties: {
    label: string
    addendum?: { pad?: { bin?: string } }
  }
}

type BuildingInputMode = 'address' | 'nn' | 'bin'

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

  // address autocomplete
  const [addressQuery, setAddressQuery] = useState('')
  const [suggestions, setSuggestions] = useState<GeoFeature[]>([])
  const [showSuggestions, setShowSuggestions] = useState(false)
  const [binFromAddress, setBinFromAddress] = useState<string | null>(null)

  // NN / install resolution
  const [nnQuery, setNnQuery] = useState('')
  const [nnResolved, setNnResolved] = useState<{ bin: string; kind: 'nn' | 'install' } | null>(null)
  const [nnError, setNnError] = useState<string | null>(null)
  const [nnLoading, setNnLoading] = useState(false)

  const [inputMode, setInputMode] = useState<BuildingInputMode>('address')

  const addressDebounce = useRef<ReturnType<typeof setTimeout> | null>(null)
  const nnDebounce = useRef<ReturnType<typeof setTimeout> | null>(null)
  const dropdownRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowSuggestions(false)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  const handleAddressInput = (e: ChangeEvent<HTMLInputElement>) => {
    const query = e.target.value
    setAddressQuery(query)
    setBinFromAddress(null)

    if (addressDebounce.current) clearTimeout(addressDebounce.current)
    if (query.length < 3) { setSuggestions([]); setShowSuggestions(false); return }

    addressDebounce.current = setTimeout(async () => {
      try {
        const res = await fetch(
          `https://geosearch.planninglabs.nyc/v2/autocomplete?text=${encodeURIComponent(query)}`
        )
        const data = await res.json()
        const features: GeoFeature[] = (data.features ?? []).filter(
          (f: GeoFeature) => f.properties.addendum?.pad?.bin
        )
        setSuggestions(features)
        setShowSuggestions(features.length > 0)
      } catch { /* ignore */ }
    }, 250)
  }

  const selectSuggestion = (feature: GeoFeature) => {
    setBinFromAddress(feature.properties.addendum!.pad!.bin!)
    setAddressQuery(feature.properties.label)
    setShowSuggestions(false)
  }

  const handleNnInput = (e: ChangeEvent<HTMLInputElement>) => {
    const query = e.target.value
    setNnQuery(query)
    setNnResolved(null)
    setNnError(null)

    if (nnDebounce.current) clearTimeout(nnDebounce.current)
    const num = parseInt(query)
    if (!query || isNaN(num)) return

    setNnLoading(true)
    nnDebounce.current = setTimeout(async () => {
      try {
        const res = await fetch(`/api/meshdb/resolve-number/${num}`)
        if (res.ok) {
          const data = await res.json()
          setNnResolved(data)
          setNnError(null)
        } else if (res.status === 404) {
          setNnError('Not a recognized NN or install number')
        } else {
          setNnError('Resolution failed — check server configuration')
        }
      } catch {
        setNnError('Could not reach server')
      } finally {
        setNnLoading(false)
      }
    }, 500)
  }

  const set = (field: keyof FormFieldValues) => (e: ChangeEvent<HTMLInputElement>) =>
    setValues(v => ({ ...v, [field]: e.target.value }))

  const switchMode = (mode: BuildingInputMode) => {
    setInputMode(mode)
    setAddressQuery(''); setBinFromAddress(null)
    setNnQuery(''); setNnResolved(null); setNnError(null); setNnLoading(false)
    setSuggestions([]); setShowSuggestions(false)
  }

  const handleSubmit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setError('')

    let effectiveBin: string
    if (inputMode === 'address') {
      if (!binFromAddress) { setError('Select a building from the address search'); return }
      effectiveBin = binFromAddress
    } else if (inputMode === 'bin') {
      if (!values.bin_id.trim()) { setError('BIN is required'); return }
      effectiveBin = values.bin_id.trim()
    } else {
      if (!nnResolved) {
        if (nnError) { setError(nnError); return }
        if (nnLoading) { setError('Still resolving — please wait'); return }
        setError('Enter a valid NN or install number'); return
      }
      effectiveBin = nnResolved.bin
    }

    const { lat, lon, alt_m, frequency_ghz, mast_offset_ft, sample_spacing } = values
    const latF = parseFloat(lat), lonF = parseFloat(lon), altF = parseFloat(alt_m)
    if (isNaN(latF) || isNaN(lonF) || isNaN(altF)) { setError('Antenna coordinates are required'); return }

    setSubmitting(true)
    try {
      await onSubmit({
        bin_id: effectiveBin,
        lat: latF, lon: lonF, alt_m: altF,
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
            {/* Primary mode tab strip */}
            {inputMode !== 'bin' && (
              <div style={styles.tabStrip}>
                {(['address', 'nn'] as const).map(mode => (
                  <button key={mode} type="button" disabled={submitting}
                    style={{ ...styles.tab, ...(inputMode === mode ? styles.tabActive : {}) }}
                    onClick={() => switchMode(mode)}>
                    {mode === 'address' ? 'Address' : 'NN / Install #'}
                  </button>
                ))}
              </div>
            )}

            {inputMode === 'address' && (
              <>
                <div style={{ position: 'relative' }} ref={dropdownRef}>
                  <input
                    style={{ ...styles.input, ...(binFromAddress ? styles.inputConfirmed : {}) }}
                    placeholder="e.g. 120 Broadway, Manhattan"
                    value={addressQuery}
                    onChange={handleAddressInput}
                    disabled={submitting}
                    autoComplete="off"
                  />
                  {showSuggestions && (
                    <div style={styles.dropdown}>
                      {suggestions.map((f, i) => (
                        <div
                          key={i}
                          style={styles.dropdownItem}
                          onMouseDown={() => selectSuggestion(f)}
                          onMouseEnter={e => (e.currentTarget.style.background = 'rgba(255,255,255,0.06)')}
                          onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
                        >
                          {f.properties.label}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
                {binFromAddress && (
                  <div style={styles.resolvedBadge}>
                    BIN <span style={styles.resolvedValue}>{binFromAddress}</span>
                  </div>
                )}
              </>
            )}

            {inputMode === 'nn' && (
              <>
                <input
                  style={{
                    ...styles.input,
                    ...(nnResolved ? styles.inputConfirmed : {}),
                    ...(nnError ? styles.inputError : {}),
                  }}
                  type="number" min="1" step="1"
                  placeholder="e.g. 492"
                  value={nnQuery}
                  onChange={handleNnInput}
                  disabled={submitting}
                />
                {nnLoading && <div style={styles.resolvedBadge}>Resolving…</div>}
                {nnResolved && !nnLoading && (
                  <div style={styles.resolvedBadge}>
                    {nnResolved.kind === 'nn' ? 'Node' : 'Install'}{' '}
                    <span style={styles.resolvedValue}>{nnQuery}</span>
                    {' → BIN '}
                    <span style={styles.resolvedValue}>{nnResolved.bin}</span>
                  </div>
                )}
                {nnError && !nnLoading && (
                  <div style={{ ...styles.resolvedBadge, color: '#ff6b6b' }}>{nnError}</div>
                )}
              </>
            )}

            {inputMode === 'bin' && (
              <>
                <Field label="BIN" hint="Building Identification Number">
                  <input style={styles.input} placeholder="e.g. 1058335"
                    value={values.bin_id} onChange={set('bin_id')} disabled={submitting} />
                </Field>
              </>
            )}

            <div style={styles.darkPatternRow}>
              {inputMode === 'bin'
                ? <button type="button" style={styles.darkPatternLink} disabled={submitting}
                    onClick={() => switchMode('address')}>Search by address or NN/install instead</button>
                : <button type="button" style={styles.darkPatternLink} disabled={submitting}
                    onClick={() => switchMode('bin')}>Enter BIN directly</button>
              }
            </div>
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
    flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center',
    padding: 24, overflowY: 'auto',
  },
  card: {
    width: '100%', maxWidth: 560,
    background: 'rgba(22,27,34,0.9)',
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 10, padding: '32px 36px',
  },
  title: {
    fontSize: 22, fontWeight: 600, color: '#e6edf3',
    marginBottom: 6, letterSpacing: '-0.01em',
  },
  subtitle: { fontSize: 13, color: '#484f58', fontFamily: 'monospace', marginBottom: 28 },
  form: { display: 'flex', flexDirection: 'column' },
  row: { display: 'flex', gap: 12 },
  input: {
    width: '100%',
    background: 'rgba(13,17,23,0.8)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 6, color: '#e6edf3',
    fontSize: 13, fontFamily: 'monospace',
    padding: '8px 10px', outline: 'none',
    transition: 'border-color 0.15s',
    boxSizing: 'border-box',
  },
  inputConfirmed: { borderColor: 'rgba(56,139,253,0.4)' },
  inputError: { borderColor: 'rgba(255,107,107,0.4)' },
  dropdown: {
    position: 'absolute', top: '100%', left: 0, right: 0, zIndex: 100,
    background: 'rgba(22,27,34,0.98)',
    border: '1px solid rgba(255,255,255,0.12)', borderTop: 'none',
    borderRadius: '0 0 6px 6px', maxHeight: 220, overflowY: 'auto',
  },
  dropdownItem: {
    padding: '8px 10px', fontSize: 12, fontFamily: 'monospace',
    color: '#c9d1d9', cursor: 'pointer', background: 'transparent',
    transition: 'background 0.1s',
    borderBottom: '1px solid rgba(255,255,255,0.04)',
  },
  tabStrip: {
    display: 'flex', gap: 2, marginBottom: 10,
  },
  tab: {
    flex: 1, padding: '5px 0',
    background: 'rgba(13,17,23,0.6)',
    border: '1px solid rgba(255,255,255,0.08)',
    borderRadius: 5, color: '#484f58',
    fontSize: 11, fontFamily: 'monospace',
    cursor: 'pointer', transition: 'all 0.15s',
    letterSpacing: '0.02em',
  },
  tabActive: {
    background: 'rgba(31,111,235,0.15)',
    borderColor: 'rgba(56,139,253,0.3)',
    color: '#388bfd',
  },
  resolvedBadge: { marginTop: 6, fontSize: 11, fontFamily: 'monospace', color: '#484f58' },
  resolvedValue: { color: '#388bfd', marginLeft: 4 },
  darkPatternRow: {
    marginTop: 8, display: 'flex', justifyContent: 'flex-end',
  },
  darkPatternLink: {
    background: 'none', border: 'none', padding: 0, cursor: 'pointer',
    fontSize: 10, fontFamily: 'monospace', color: '#3d444d',
    letterSpacing: '0.02em',
  },
  error: { color: '#ff4444', fontSize: 12, fontFamily: 'monospace', marginBottom: 12 },
  submit: {
    marginTop: 4, padding: '10px 0', borderRadius: 6,
    background: 'linear-gradient(135deg, #1f6feb, #388bfd)',
    color: '#fff', fontSize: 14, fontFamily: 'monospace',
    fontWeight: 600, cursor: 'pointer', border: 'none',
    letterSpacing: '0.03em', transition: 'opacity 0.15s',
  },
}
