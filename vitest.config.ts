import { defineConfig } from 'vitest/config'

// テストは tools/ と packages/・apps/ の *.test.ts と *.test.tsx。Rust の target/ と依存は見ない
export default defineConfig({
  test: {
    include: [
      'tools/**/*.test.ts',
      'packages/*/src/**/*.test.{ts,tsx}',
      'packages/*/test/**/*.test.{ts,tsx}',
      'apps/*/src/**/*.test.{ts,tsx}',
    ],
    exclude: ['**/node_modules/**', 'target/**'],
    // テストからネットワークに出ない (#11)。127.0.0.1・localhost のほかへ出ようとしたら断る
    setupFiles: ['tools/no-network.setup.ts'],
  },
})
