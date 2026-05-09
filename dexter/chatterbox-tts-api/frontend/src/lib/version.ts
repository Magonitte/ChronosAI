/**
 * Frontend version utilities
 */

// This will be set at build time by Vite
export const FRONTEND_VERSION = '2.1.0'; // Fallback version

/**
 * Get the frontend version
 */
export function getFrontendVersion(): string {
  // In production, this could be injected by the build process
  // For now, use the static version
  return FRONTEND_VERSION;
}

/**
 * Get comprehensive frontend version info
 */
export function getFrontendVersionInfo() {
  return {
    version: getFrontendVersion(),
    name: 'Chatterbox TTS Frontend',
    build_date: new Date().toISOString().split('T')[0] // Just date, not full timestamp
  };
} 