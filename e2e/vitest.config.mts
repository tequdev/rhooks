import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    globals: true,
    watch: false,
    // Tests share standalone ledger state.
    fileParallelism: false,
    sequence: {
      concurrent: false,
    },
    // SetHook and ledger operations can take longer than the default timeout.
    testTimeout: 60_000,
    hookTimeout: 60_000,
  },
})
