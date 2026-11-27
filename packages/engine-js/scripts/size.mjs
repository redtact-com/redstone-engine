// 大きさの報告 (合否にしない。19 §11 の 3)。
//
// - wasm と状態表の JSON は generated/ の物 (先に npm run build:engine)。
// - JS は src/ のうちブラウザの束に入る物 (node.ts とテストを除く) を、TypeScript で型と注を外して
//   足した大きさ。縮め (minify) はしていない。束ねる道具はアプリの PR で入る。
// GITHUB_STEP_SUMMARY があれば、表をそこへ足す。
import { appendFileSync, readFileSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { brotliCompressSync, constants, gzipSync } from 'node:zlib'
import ts from 'typescript'

const pkg = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const src = join(pkg, 'src')

const browserFiles = readdirSync(src)
  .filter(f => f.endsWith('.ts') && !f.endsWith('.test.ts') && f !== 'node.ts')
  .sort()
const js = browserFiles
  .map(f =>
    ts.transpileModule(readFileSync(join(src, f), 'utf-8'), {
      compilerOptions: { target: ts.ScriptTarget.ES2023, module: ts.ModuleKind.ESNext, removeComments: true },
    }).outputText,
  )
  .join('\n')

const rows = [
  ['rse_wasm.wasm', readFileSync(join(pkg, 'generated', 'rse_wasm.wasm'))],
  ['states.json', readFileSync(join(pkg, 'generated', 'states.json'))],
  [`engine-js の JS (${browserFiles.join('・')}。型と注を外す・縮めない)`, Buffer.from(js)],
].map(([name, bytes]) => {
  const gz = gzipSync(bytes, { level: 9 }).length
  const br = brotliCompressSync(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length
  return `| ${name} | ${bytes.length} | ${gz} | ${br} |`
})

const table = ['## 大きさ (バイト。合否にしない)', '', '| 物 | 素 | gzip | brotli |', '|---|---:|---:|---:|', ...rows, ''].join('\n')
console.log(table)
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${table}\n`)
