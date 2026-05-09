import { MOBILE_BREAKPOINT } from '@/config/constants'
import * as React from "react"

export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState<boolean>(false)

  React.useEffect(() => {
    const checkIsMobile = () => {
      return window.innerWidth < MOBILE_BREAKPOINT
    }

    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)

    // Set initial state
    setIsMobile(checkIsMobile())

    const onChange = () => {
      setIsMobile(checkIsMobile())
    }

    mql.addEventListener("change", onChange)

    // Also listen to resize events for more reliable detection
    window.addEventListener('resize', onChange)

    return () => {
      mql.removeEventListener("change", onChange)
      window.removeEventListener('resize', onChange)
    }
  }, [])

  return isMobile
}
