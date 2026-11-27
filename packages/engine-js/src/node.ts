/**
 * Node で、ファイルから読む (テストと道具)。ブラウザの束には入れない (`@redstone-engine/engine-js/node`)。
 *
 * 既定の場所は、このパッケージの generated/ (`npm run build:engine` が wasm と状態表の JSON を置く。追跡しない)。
 */

import { readFile } from 'node:fs/promises'
import { Engine } from './engine'

/** `npm run build:engine` が書く場所 */
export const GENERATED_DIR = new URL('../generated/', import.meta.url)

/** `dir` の rse_wasm.wasm と states.json から読む */
export async function loadEngineFromFiles(dir: URL = GENERATED_DIR): Promise<Engine> {
  const wasmFile = new URL('rse_wasm.wasm', dir)
  const statesFile = new URL('states.json', dir)
  let files: [Uint8Array, string]
  try {
    files = await Promise.all([readFile(wasmFile), readFile(statesFile, 'utf-8')])
  } catch (e) {
    throw new Error(`wasm か状態表の JSON が読めない (npm run build:engine で作る): ${(e as Error).message}`)
  }
  return Engine.load(files[0], files[1])
}
