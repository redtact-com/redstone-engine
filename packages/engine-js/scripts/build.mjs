// wasm と状態表の JSON を作り、packages/engine-js/generated/ に置く (生成物。追跡しない)。
//
// `npm run build:engine` (と、`npm test` の前の pretest) が呼ぶ。cargo が要る。
// テストが古い wasm で回らないよう、毎回 cargo を呼ぶ (変わっていなければ cargo はすぐ終わる)。
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const pkg = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const root = resolve(pkg, '..', '..')
const out = join(pkg, 'generated')
const target = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'target')

const cargo = (...args) => execFileSync('cargo', args, { cwd: root, stdio: 'inherit' })

mkdirSync(out, { recursive: true })
cargo('build', '-p', 'rse-wasm', '--target', 'wasm32-unknown-unknown', '--release')
copyFileSync(join(target, 'wasm32-unknown-unknown', 'release', 'rse_wasm.wasm'), join(out, 'rse_wasm.wasm'))
cargo('run', '-q', '-p', 'rse-cli', '--', 'states', join(out, 'states.json'))
console.log('engine-js: generated/rse_wasm.wasm と generated/states.json を作った')
