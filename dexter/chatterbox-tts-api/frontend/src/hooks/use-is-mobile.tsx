// useIsMobile.js
import { MOBILE_BREAKPOINT } from '@/config/constants';
import { useState, useEffect } from 'react';

/**
 * A custom React hook to determine if the current device screen width is
 * considered "mobile".
 *
 * @param {number} [breakpoint=768] The maximum width for a screen to be
 * considered mobile. Defaults to 768px.
 * @returns {boolean} True if the window width is less than or equal to the
 * breakpoint, false otherwise.
 */
const useIsMobile = (breakpoint: number = MOBILE_BREAKPOINT): boolean => {
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    // Check if the window object is defined (prevents errors during SSR)
    if (typeof window === 'undefined') {
      return;
    }

    const handleResize = () => {
      // Set the isMobile state based on the window width
      setIsMobile(window.innerWidth <= breakpoint);
    };

    // Call handler right away so state gets updated with initial window size
    handleResize();

    // Add event listener to handle window resizing
    window.addEventListener('resize', handleResize);

    // Clean up the event listener when the component unmounts
    return () => {
      window.removeEventListener('resize', handleResize);
    };
  }, [breakpoint]); // Re-run effect if breakpoint changes

  return isMobile;
};

export default useIsMobile;