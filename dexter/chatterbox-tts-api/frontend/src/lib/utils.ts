import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * Extract duration from an audio blob using the browser's Audio API
 * @param blob - The audio blob to analyze
 * @returns Promise that resolves to duration in seconds, or null if extraction fails
 */
export async function extractAudioDuration(blob: Blob): Promise<number | null> {
  return new Promise((resolve) => {
    try {
      const audio = new Audio()
      const objectUrl = URL.createObjectURL(blob)

      audio.addEventListener('loadedmetadata', () => {
        URL.revokeObjectURL(objectUrl)
        resolve(audio.duration)
      })

      audio.addEventListener('error', () => {
        URL.revokeObjectURL(objectUrl)
        resolve(null)
      })

      // Set a timeout to avoid hanging indefinitely
      const timeout = setTimeout(() => {
        URL.revokeObjectURL(objectUrl)
        resolve(null)
      }, 5000) // 5 second timeout

      audio.addEventListener('loadedmetadata', () => {
        clearTimeout(timeout)
      })

      audio.src = objectUrl
    } catch (error) {
      console.error('Error extracting audio duration:', error)
      resolve(null)
    }
  })
}

/**
 * Format duration in seconds to MM:SS or HH:MM:SS format
 * @param duration - Duration in seconds
 * @returns Formatted duration string
 */
export function formatDuration(duration: number): string {
  if (isNaN(duration) || duration < 0) {
    return "0:00"
  }

  const hours = Math.floor(duration / 3600)
  const minutes = Math.floor((duration % 3600) / 60)
  const seconds = Math.floor(duration % 60)

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`
  }

  return `${minutes}:${seconds.toString().padStart(2, '0')}`
}

/**
 * Checks if a URL string ends with '/v1' and removes it if present.
 *
 * @param url The input URL string.
 * @returns The URL string without the '/v1' suffix.
 */
export const removeV1Suffix = (url: string): string => {
  const suffix = '/v1';

  if (url.endsWith(suffix)) {
    // Return the string sliced from the beginning up to the start of the suffix.
    return url.slice(0, -suffix.length);
  }

  // If the suffix isn't found, return the original url unchanged.
  return url;
}

/**
 * Removes the decimal part from a string or number and returns the integer part as a string.
 *
 * @param value The input string or number (e.g., "33.33" or 45.9).
 * @returns The integer part of the value as a string (e.g., "33", "45").
 */
export const removeDecimal = (value: string | number): string => {
  // 1. Ensure the input is a string to handle both types uniformly.
  const stringValue = String(value);

  // 2. Split the string at the decimal point. This returns an array.
  //    - "33.33" becomes ["33", "33"]
  //    - "100" becomes ["100"]
  const parts = stringValue.split('.');

  // 3. The part before the decimal point is always the first element of the array.
  return parts[0];
}

/**
 * Format file size in bytes to human readable format (B, KB, MB, GB)
 * @param bytes - File size in bytes
 * @returns Formatted file size string
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';

  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}