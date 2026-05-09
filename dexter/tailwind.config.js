/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      animation: {
        'breathe': 'breathe 3.5s ease-in-out infinite',
        'breathe-fast': 'breathe 1.4s ease-in-out infinite',
        'pulse-slow': 'pulse-glow 0.9s ease-in-out infinite',
        'spin-slow': 'spin 3s linear infinite',
        'spin-medium': 'spin 2s linear infinite',
        'spin-fast': 'spin 1.5s linear infinite',
        'speak-pulse': 'speak-pulse 0.6s ease-in-out infinite',
        'ring-pulse': 'ring-pulse 1.2s ease-in-out infinite',
        'bubble-in': 'bubble-in 0.3s cubic-bezier(0.34, 1.56, 0.64, 1)',
        'fade-in': 'fade-in 0.4s cubic-bezier(0.34, 1.56, 0.64, 1)',
        'slide-up': 'slide-up 0.35s cubic-bezier(0.34, 1.56, 0.64, 1)',
        'gear-spin': 'spin 2s linear infinite',
      },
      keyframes: {
        'breathe': {
          '0%, 100%': { transform: 'scale(1)', opacity: '0.5' },
          '50%': { transform: 'scale(1.12)', opacity: '0.9' },
        },
        'pulse-glow': {
          '0%, 100%': { transform: 'scale(1)', opacity: '0.6' },
          '50%': { transform: 'scale(1.25)', opacity: '1' },
        },
        'speak-pulse': {
          '0%, 100%': { transform: 'scale(1.02)', opacity: '0.7' },
          '50%': { transform: 'scale(1.2)', opacity: '1' },
        },
        'ring-pulse': {
          '0%, 100%': { transform: 'scale(1)', opacity: '0.2' },
          '50%': { transform: 'scale(1.08)', opacity: '0.5' },
        },
        'bubble-in': {
          from: { opacity: '0', transform: 'translateY(10px) scale(0.92)' },
          to: { opacity: '1', transform: 'translateY(0) scale(1)' },
        },
        'fade-in': {
          from: { opacity: '0', transform: 'scale(0.9)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'slide-up': {
          from: { opacity: '0', transform: 'translateY(6px) scale(0.9)' },
          to: { opacity: '1', transform: 'translateY(0) scale(1)' },
        },
      },
    },
  },
  plugins: [],
}
