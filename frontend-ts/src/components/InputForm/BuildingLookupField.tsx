/**
 * Reusable building identifier field.
 * Supports address autocomplete, NN/install number resolution, and direct BIN entry.
 * Calls onBinResolved(bin, label) when a valid BIN is confirmed, onBinCleared() when cleared.
 */
import { useState, useRef, useEffect } from 'react'
import type { ChangeEvent } from 'react'

interface GeoFeature {
  properties: {
    label: string
    addendum?: { pad?: { bin?: string } }
  }
}

type LookupMode = 'address' | 'nn' | 'bin'

interface BuildingLookupFieldProps {
  disabled?: boolean
  onBinResolved: (bin: string, label: string) => void
  onBinCleared: () => void
}

export default function BuildingLookupField({ disabled, onBinResolved, onBinCleared }: BuildingLookupFieldProps) {
  const [inputMode, setInputMode] = useState<LookupMode>('address')

  const [addressQuery, setAddressQuery]     = useState('')
  const [suggestions, setSuggestions]       = useState<GeoFeature[]>([])
  const [showSuggestions, setShowSuggestions] = useState(false)
  const [binFromAddress, setBinFromAddress] = useState<string | null>(null)

  const [nnQuery, setNnQuery]       = useState('')
  const [nnResolved, setNnResolved] = useState<{ bin: string; kind: 'nn' | 'install' } | null>(null)
  const [nnError, setNnError]       = useState<string | null>(null)
  const [nnLoading, setNnLoading]   = useState(false)

  const [binValue, setBinValue] = useState('')

  const addressDebounce = useRef<ReturnType<typeof setTimeout> | null>(null)
  const nnDebounce      = useRef<ReturnType<typeof setTimeout> | null>(null)
  const dropdownRef     = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node))
        setShowSuggestions(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  const clearAll = () => {
    setAddressQuery(''); setBinFromAddress(null)
    setNnQuery(''); setNnResolved(null); setNnError(null); setNnLoading(false)
    setBinValue('')
    setSuggestions([]); setShowSuggestions(false)
    onBinCleared()
  }

  const switchMode = (mode: LookupMode) => { setInputMode(mode); clearAll() }

  const handleAddressInput = (e: ChangeEvent<HTMLInputElement>) => {
    const query = e.target.value
    setAddressQuery(query)
    if (binFromAddress) { setBinFromAddress(null); onBinCleared() }
    if (addressDebounce.current) clearTimeout(addressDebounce.current)
    if (query.length < 3) { setSuggestions([]); setShowSuggestions(false); return }
    addressDebounce.current = setTimeout(async () => {
      try {
        const res  = await fetch(`https://geosearch.planninglabs.nyc/v2/autocomplete?text=${encodeURIComponent(query)}`)
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
    const bin = feature.properties.addendum!.pad!.bin!
    setBinFromAddress(bin)
    setAddressQuery(feature.properties.label)
    setShowSuggestions(false)
    onBinResolved(bin, feature.properties.label)
  }

  const handleNnInput = (e: ChangeEvent<HTMLInputElement>) => {
    const query = e.target.value
    setNnQuery(query)
    if (nnResolved) { setNnResolved(null); onBinCleared() }
    setNnError(null)
    if (nnDebounce.current) clearTimeout(nnDebounce.current)
    const num = parseInt(query)
    if (!query || isNaN(num)) { setNnLoading(false); return }
    setNnLoading(true)
    nnDebounce.current = setTimeout(async () => {
      try {
        const res = await fetch(`/api/meshdb/resolve-number/${num}`)
        if (res.ok) {
          const data = await res.json()
          setNnResolved(data)
          const label = data.kind === 'nn'
            ? `NN ${query} (BIN ${data.bin})`
            : `Install ${query} (BIN ${data.bin})`
          onBinResolved(data.bin, label)
        } else if (res.status === 404) {
          setNnError('Not a recognized NN or install number'); onBinCleared()
        } else {
          setNnError('Resolution failed — check server configuration'); onBinCleared()
        }
      } catch {
        setNnError('Could not reach server'); onBinCleared()
      } finally {
        setNnLoading(false)
      }
    }, 500)
  }

  const handleBinInput = (e: ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value
    setBinValue(v)
    v.trim() ? onBinResolved(v.trim(), `BIN ${v.trim()}`) : onBinCleared()
  }

  return (
    <>
      {inputMode !== 'bin' && (
        <div style={s.tabStrip}>
          {(['address', 'nn'] as const).map(mode => (
            <button key={mode} type="button" disabled={disabled}
              style={{ ...s.tab, ...(inputMode === mode ? s.tabActive : {}) }}
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
              style={{ ...s.input, ...(binFromAddress ? s.inputConfirmed : {}) }}
              placeholder="e.g. 120 Broadway, Manhattan"
              value={addressQuery}
              onChange={handleAddressInput}
              disabled={disabled}
              autoComplete="off"
            />
            {showSuggestions && (
              <div style={s.dropdown}>
                {suggestions.map((f, i) => (
                  <div key={i} style={s.dropdownItem}
                    onMouseDown={() => selectSuggestion(f)}
                    onMouseEnter={e => (e.currentTarget.style.background = 'rgba(255,255,255,0.06)')}
                    onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}>
                    {f.properties.label}
                  </div>
                ))}
              </div>
            )}
          </div>
          {binFromAddress && (
            <div style={s.resolvedBadge}>
              BIN <span style={s.resolvedValue}>{binFromAddress}</span>
            </div>
          )}
        </>
      )}

      {inputMode === 'nn' && (
        <>
          <input
            style={{ ...s.input, ...(nnResolved ? s.inputConfirmed : {}), ...(nnError ? s.inputError : {}) }}
            type="number" min="1" step="1" placeholder="e.g. 492"
            value={nnQuery} onChange={handleNnInput} disabled={disabled}
          />
          {nnLoading && <div style={s.resolvedBadge}>Resolving…</div>}
          {nnResolved && !nnLoading && (
            <div style={s.resolvedBadge}>
              {nnResolved.kind === 'nn' ? 'Node' : 'Install'}{' '}
              <span style={s.resolvedValue}>{nnQuery}</span>{' → BIN '}
              <span style={s.resolvedValue}>{nnResolved.bin}</span>
            </div>
          )}
          {nnError && !nnLoading && (
            <div style={{ ...s.resolvedBadge, color: '#ff6b6b' }}>{nnError}</div>
          )}
        </>
      )}

      {inputMode === 'bin' && (
        <input style={s.input} placeholder="e.g. 1058335"
          value={binValue} onChange={handleBinInput} disabled={disabled} />
      )}

      <div style={s.darkPatternRow}>
        {inputMode === 'bin'
          ? <button type="button" style={s.darkPatternLink} disabled={disabled}
              onClick={() => switchMode('address')}>Search by address or NN/install instead</button>
          : <button type="button" style={s.darkPatternLink} disabled={disabled}
              onClick={() => switchMode('bin')}>Enter BIN directly</button>
        }
      </div>
    </>
  )
}

export const lookupFieldStyles: Record<string, React.CSSProperties> = {
  input: {
    width: '100%', background: 'rgba(13,17,23,0.8)',
    border: '1px solid rgba(255,255,255,0.1)', borderRadius: 6,
    color: '#e6edf3', fontSize: 13, fontFamily: 'monospace',
    padding: '8px 10px', outline: 'none', transition: 'border-color 0.15s',
    boxSizing: 'border-box',
  },
  inputConfirmed: { borderColor: 'rgba(56,139,253,0.4)' },
  inputError:     { borderColor: 'rgba(255,107,107,0.4)' },
}

const s: Record<string, React.CSSProperties> = {
  ...lookupFieldStyles,
  dropdown: {
    position: 'absolute', top: '100%', left: 0, right: 0, zIndex: 100,
    background: 'rgba(22,27,34,0.98)', border: '1px solid rgba(255,255,255,0.12)',
    borderTop: 'none', borderRadius: '0 0 6px 6px', maxHeight: 220, overflowY: 'auto',
  },
  dropdownItem: {
    padding: '8px 10px', fontSize: 12, fontFamily: 'monospace',
    color: '#c9d1d9', cursor: 'pointer', background: 'transparent',
    borderBottom: '1px solid rgba(255,255,255,0.04)',
  },
  tabStrip:   { display: 'flex', gap: 2, marginBottom: 10 },
  tab: {
    flex: 1, padding: '5px 0', background: 'rgba(13,17,23,0.6)',
    border: '1px solid rgba(255,255,255,0.08)', borderRadius: 5,
    color: '#484f58', fontSize: 11, fontFamily: 'monospace',
    cursor: 'pointer', letterSpacing: '0.02em',
  },
  tabActive: {
    background: 'rgba(31,111,235,0.15)', borderColor: 'rgba(56,139,253,0.3)', color: '#388bfd',
  },
  resolvedBadge:  { marginTop: 6, fontSize: 11, fontFamily: 'monospace', color: '#484f58' },
  resolvedValue:  { color: '#388bfd', marginLeft: 4 },
  darkPatternRow: { marginTop: 8, display: 'flex', justifyContent: 'flex-end' },
  darkPatternLink: {
    background: 'none', border: 'none', padding: 0, cursor: 'pointer',
    fontSize: 11, fontFamily: 'monospace', color: '#5a6472', letterSpacing: '0.02em',
  },
}
