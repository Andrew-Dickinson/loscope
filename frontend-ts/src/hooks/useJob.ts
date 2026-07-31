import { useState, useEffect, useRef } from 'react'
import { fetchWithRetry, AbortedError } from '../lib/fetchWithRetry'

export interface JobState {
  status: string
  progress_pct: number
  message: string
  result: Record<string, unknown> | null
  error: string | null
}

/**
 * Poll /api/jobs/<jobId> until done or error.
 * Returns JobState or null.
 */
export function useJob(jobId: string | null): JobState | null {
  const [job, setJob] = useState<JobState | null>(null)
  const cancelRef = useRef(false)

  useEffect(() => {
    if (!jobId) {
      setJob(null)
      return
    }
    cancelRef.current = false
    setJob({ status: 'pending', progress_pct: 0, message: '', result: null, error: null })

    const poll = async () => {
      while (!cancelRef.current) {
        try {
          const res = await fetchWithRetry(`/api/jobs/${jobId}`, undefined, () => cancelRef.current)
          const data = await res.json() as JobState
          setJob(data)
          if (data.status === 'done' || data.status === 'error') return
        } catch (err) {
          if (err instanceof AbortedError) return
          setJob(j => j ? { ...j, status: 'error', error: err instanceof Error ? err.message : String(err) } : null)
          return
        }
        await new Promise(r => setTimeout(r, 800))
      }
    }
    poll()
    return () => { cancelRef.current = true }
  }, [jobId])

  return job
}
