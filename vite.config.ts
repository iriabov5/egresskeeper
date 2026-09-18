import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

/**
 * Порт dev-сервера фиксирован: он указан как `devUrl` в `src-tauri/tauri.conf.json`,
 * а политика навигации webview разрешает loopback только в dev-сборке.
 */
const DEV_SERVER_PORT = 1420;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: DEV_SERVER_PORT,
    strictPort: true,
  },
  build: {
    // Целевые движки — webview платформ: WebKit (macOS), WebView2 (Windows), WebKitGTK (Linux).
    target: 'es2022',
    sourcemap: false,
    emptyOutDir: true,
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup.ts'],
    restoreMocks: true,
    css: false,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      reportsDirectory: './target/sonar/coverage-web',
      include: ['src/**/*.{ts,tsx}'],
      exclude: ['src/**/*.test.{ts,tsx}', 'src/test/**', 'src/vite-env.d.ts'],
    },
  },
});
