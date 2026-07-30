// Shared fetch wrapper for calls into our own backend. Centralizes the retry/backoff policy so
// every endpoint that can be memory-throttled (503 + Retry-After) — not just analyzePointPair —
// gets the same treatment, instead of each component silently giving up on the first failure.

export class FetchError extends Error {
  constructor(message: string, readonly retryable: boolean, readonly status?: number) {
    super(message)
    this.name = 'FetchError'
  }
}

export class AbortedError extends Error {
  constructor() {
    super('Aborted')
    this.name = 'AbortedError'
  }
}

const MAX_RETRIES = 6
const BASE_BACKOFF_MS = 1000
const MAX_BACKOFF_MS = 30000
const ABORT_POLL_MS = 100

function backoffDelayMs(attempt: number, retryAfterMs?: number): number {
  const exponential = Math.min(BASE_BACKOFF_MS * 2 ** attempt, MAX_BACKOFF_MS)
  const jittered = exponential * (0.5 + Math.random() * 0.5)
  // Never wait less than the server's suggested Retry-After, but still apply our own backoff
  // growth on top of it for repeated throttling.
  return retryAfterMs !== undefined ? Math.max(retryAfterMs, jittered) : jittered
}

// Sleeps in short increments so an abort can interrupt a long backoff wait (up to 30s) almost
// immediately, instead of the caller having to wait out the full delay before noticing.
async function sleepAbortable(ms: number, isAborted: () => boolean): Promise<void> {
  let waited = 0
  while (waited < ms) {
    if (isAborted()) return
    const step = Math.min(ABORT_POLL_MS, ms - waited)
    await new Promise(resolve => setTimeout(resolve, step))
    waited += step
  }
}

/**
 * fetch() wrapper with exponential-backoff retry.
 *
 * - 503 (our own memory-throttling admission control): retried indefinitely, honoring the
 *   server's Retry-After header — throttling is guaranteed to clear once other in-flight
 *   requests finish, so giving up is never correct, only slow.
 * - Network errors / other 5xx: retried up to MAX_RETRIES times, since there's no such
 *   guarantee for a genuinely broken backend.
 * - 4xx: never retried — thrown immediately as a non-retryable FetchError.
 *
 * Resolves with the raw Response for any ok status (including 204) — callers parse the body
 * themselves (json/arrayBuffer/blob), since that varies per endpoint.
 *
 * `isAborted`, if given, is polled before every attempt and during backoff waits (every
 * ABORT_POLL_MS) so cancellation (e.g. a component unmounting, or a user-triggered "Stop")
 * takes effect promptly rather than after a long backoff finishes. On abort, throws
 * AbortedError — callers already guarding state updates on their own cancelled flag can let
 * this propagate and ignore it.
 */
export async function fetchWithRetry(
  input: string,
  init?: RequestInit,
  isAborted: () => boolean = () => false,
): Promise<Response> {
  for (let attempt = 0; ; attempt++) {
    if (isAborted()) throw new AbortedError()
    let res: Response
    try {
      res = await fetch(input, init)
    } catch (err) {
      if (isAborted()) throw new AbortedError()
      if (attempt >= MAX_RETRIES) throw new FetchError(String(err), true)
      await sleepAbortable(backoffDelayMs(attempt), isAborted)
      continue
    }
    if (res.ok) return res
    if (isAborted()) throw new AbortedError()
    if (res.status === 503) {
      const retryAfterHeader = res.headers.get('Retry-After')
      const retryAfterMs = retryAfterHeader ? Number(retryAfterHeader) * 1000 : undefined
      await sleepAbortable(backoffDelayMs(attempt, retryAfterMs), isAborted)
      continue
    }
    if (res.status >= 500) {
      if (attempt >= MAX_RETRIES) throw new FetchError(`HTTP ${res.status}`, true, res.status)
      await sleepAbortable(backoffDelayMs(attempt), isAborted)
      continue
    }
    throw new FetchError(`HTTP ${res.status}`, false, res.status)
  }
}
