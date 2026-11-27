import { defineConfig } from 'vitest/config'

// テストは tools/ と packages/・apps/ の *.test.ts。Rust の target/ と依存は見ない
export default defineConfig({
  test: {
    include: ['tools/**/*.test.ts', 'packages/*/src/**/*.test.ts', 'packages/*/test/**/*.test.ts', 'apps/*/src/**/*.test.ts'],
    exclude: ['**/node_modules/**', 'target/**'],
  },
})
