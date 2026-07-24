import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    globals: true,
    watch: false,
    // Tests share standalone ledger state (same wallets, sequential
    // ledger_accept calls) - fileParallelism/concurrency must stay off,
    // matching every sibling hook repo's vitest config.
    fileParallelism: false,
    sequence: {
      concurrent: false,
    },
    // Node operations under standalone + (on Apple Silicon) amd64
    // emulation are slow; the sibling repos' default 5s timeout is too
    // tight for SetHook + multiple ledger_accept round-trips.
    testTimeout: 60_000,
    hookTimeout: 60_000,
  },
})
