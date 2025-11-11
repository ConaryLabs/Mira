// tailwind.config.js
/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      animation: {
        'fade-in': 'fadeIn 0.3s ease-out',
        'slide-in': 'slideIn 0.5s ease-out',
        'slide-in-right': 'slideInRight 0.3s ease-out',
        'pulse-slow': 'pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite',
        'bounce-delayed': 'bounce 1.4s infinite ease-in-out',
        'shimmer': 'shimmer 2s infinite',
        'typing-dots': 'typingDots 1.5s infinite ease-in-out',
      },
      keyframes: {
        fadeIn: {
          'from': { opacity: 0, transform: 'translateY(10px)' },
          'to': { opacity: 1, transform: 'translateY(0)' },
        },
        slideIn: {
          'from': { opacity: 0, transform: 'translateY(20px)' },
          'to': { opacity: 1, transform: 'translateY(0)' },
        },
        slideInRight: {
          'from': { opacity: 0, transform: 'translateX(100%)' },
          'to': { opacity: 1, transform: 'translateX(0)' },
        },
        shimmer: {
          '0%': { backgroundPosition: '-200% 0' },
          '100%': { backgroundPosition: '200% 0' },
        },
        typingDots: {
          '0%, 60%, 100%': { 
            transform: 'scale(0.8)',
            opacity: 0.5 
          },
          '30%': { 
            transform: 'scale(1.2)',
            opacity: 1 
          },
        },
      },
      // NOTE: We use inline styles for animation delays (e.g., style={{ animationDelay: '150ms' }})
      // This is cleaner than adding Tailwind utilities and avoids CSS generation issues
    },
  },
  plugins: [],
}
