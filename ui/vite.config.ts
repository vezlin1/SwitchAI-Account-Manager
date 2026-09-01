import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    target: 'esnext',
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/react/') || id.includes('node_modules/react-dom/')) {
            return 'vendor-react'
          }
          if (id.includes('node_modules/@dnd-kit/')) {
            return 'vendor-dnd'
          }
          if (id.includes('node_modules/@tauri-apps/')) {
            return 'vendor-tauri'
          }
          if (id.includes('node_modules/lucide-react/')) {
            return 'vendor-icons'
          }
        }
      }
    }
  }
})
