/**
 * Node で、ファイルから読む (`@redstone-engine/engine-js/node` の入口)。vitest や束ねる道具 (Vite ほか) の中で読む。
 * パッケージの exports が .ts を指し、import に拡張子が無いので、素の node では読めない。
 * node:fs を使うので、ブラウザの束には入れない。
 *
 * 既定の場所は、このパッケージの generated/ (`npm run build:engine` が wasm と状態表の JSON を置く。追跡しない)。
 */

import { readFile } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'
import { Engine } from './engine'

/** `npm run build:engine` が書く場所 */
export const GENERATED_DIR = new URL('../generated/', import.meta.url)

/** フォルダの URL にする。文字列はファイルの道筋として読み、末尾に `/` が無ければ足す (親のフォルダから読まない) */
function folder(dir: URL | string): URL {
  const url = typeof dir === 'string' ? pathToFileURL(dir) : dir
  return url.href.endsWith('/') ? url : new URL(`${url.href}/`)
}

/** `dir` (URL かファイルの道筋) の rse_wasm.wasm と states.json から読む */
export async function loadEngineFromFiles(dir: URL | string = GENERATED_DIR): Promise<Engine> {
  const base = folder(dir)
  const wasmFile = new URL('rse_wasm.wasm', base)
  const statesFile = new URL('states.json', base)
  let files: [Uint8Array, string]
  try {
    files = await Promise.all([readFile(wasmFile), readFile(statesFile, 'utf-8')])
  } catch (e) {
    throw new Error(`wasm か状態表の JSON が読めない (npm run build:engine で作る): ${(e as Error).message}`)
  }
  return Engine.load(files[0], files[1])
}
