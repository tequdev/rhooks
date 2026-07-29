// Wires hooks-build's example outputs into the place
// `@transia/hooks-toolkit`'s `readHookBinaryHexFromNS(name, 'wasm')` reads
// from: `${process.cwd()}/build/<name>.wasm` (see
// node_modules/@transia/hooks-toolkit/dist/npm/src/utils.js). Copying
// keeps every test's Hook-building code on the toolkit's own file-reading
// helper (matching the sibling repos' pattern) instead of bypassing it.
//
// Run before `vitest run` (wired as the `pretest` script). Requires
// `examples/*/out/*.wasm` to already exist - run `mise run build-examples`
// (or the CI `build-hooks` job) first.
import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const e2eRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const repoRoot = dirname(e2eRoot)
const buildDir = join(e2eRoot, 'build')

// example directory name (numbered - suggested reading order, see
// examples/README.md) -> wasm basename produced by hooks-build (the crate
// name with `-` replaced by `_`, per examples/*/Cargo.toml [package].name)
const examples = {
  '01_accept-all': 'accept_all',
  '02_state-counter': 'state_counter',
  '03_hook-params': 'hook_params',
  '04_errors': 'errors',
  '05_firewall': 'firewall',
  '06_guard-patterns': 'guard_patterns',
  '07_xfl-math': 'xfl_math',
  '08_slot-ledger': 'slot_ledger',
  '09_state-foreign': 'state_foreign',
  '10_emit-txn': 'emit_txn',
  '13_keylets': 'keylets',
  '14_account-id-macro': 'account_id_macro',
  '80_reward': 'reward',
  '81_govern': 'govern',
}

mkdirSync(buildDir, { recursive: true })

for (const [exampleDir, wasmName] of Object.entries(examples)) {
  const src = join(repoRoot, 'examples', exampleDir, 'out', `${wasmName}.wasm`)
  if (!existsSync(src)) {
    console.error(
      `error: ${src} not found. Build the examples first: mise run build-examples`,
    )
    process.exit(1)
  }
  const dest = join(buildDir, `${wasmName}.wasm`)
  copyFileSync(src, dest)
  console.log(`copied ${src} -> ${dest}`)
}
