'use client'

import { useEffect, useState } from 'react'

/**
 * Custom hook to detect when component has mounted on client-side
 * Useful for preventing hydration mismatches when using localStorage or other client-only APIs
 */
export function useMounted() {
  const [mounted, setMounted] = useState(false)

  useEffect(() => {
    setMounted(true)
  }, [])

  return mounted
} 