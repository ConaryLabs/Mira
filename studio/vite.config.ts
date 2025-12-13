import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit(), tailwindcss()],
  server: {
    allowedHosts: ['mira.conarylabs.com', 'localhost'],
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true
      },
      '/mcp': {
        target: 'http://localhost:3000',
        changeOrigin: true
      }
    }
  }
});
