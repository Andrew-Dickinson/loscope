import { useState, useEffect } from 'react'

// ~30% below the breakpoint InputForm's footer uses for its own layout switch (900px) —
// this hook's consumers (compass/mini-map overlays) need less room than a full form layout.
const BREAKPOINT_PX = 630

export function useIsSmallScreen(): boolean {
  const [isSmall, setIsSmall] = useState(() => window.matchMedia(`(max-width: ${BREAKPOINT_PX}px)`).matches)

  useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${BREAKPOINT_PX}px)`)
    const onChange = () => setIsSmall(mql.matches)
    mql.addEventListener('change', onChange)
    return () => mql.removeEventListener('change', onChange)
  }, [])

  return isSmall
}
