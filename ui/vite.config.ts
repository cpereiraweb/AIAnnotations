/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { svelteTesting } from '@testing-library/svelte/vite';

// Tauri serves the dev server at a fixed port (devUrl in crates/app/tauri.conf.json).
export default defineConfig({
  // `svelteTesting` resolves Svelte's browser build and cleans up after each component test.
  plugins: [tailwindcss(), svelte(), svelteTesting()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: 'es2022',
  },
  test: {
    // Pure-logic tests run in node; component tests opt in with `@vitest-environment jsdom`.
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
