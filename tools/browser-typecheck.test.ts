import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

/**
 * ブラウザの入口の型検査 (tsconfig.browser.json。#14) が効いていることの見張り。
 *
 * 効き目は `"types": []` と、対象のファイルが @types/node をたどらないことに乗っている。どちらかが崩れても
 * (`"types": []` を消してルートの `types: ["node"]` を継ぐ・対象のファイルが vite の型を読む) `npm run typecheck` は
 * 黙って通るので、設定を型検査器で読み、(a) 読むファイルに @types/node が無いこと、(b) Node の物を書いた仮のファイルが
 * 落ちること、を見る。ファイルの中の三重スラッシュの指示は設定では止まらないので、各パッケージの依存の見張りが拾う。
 */

const REPO_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..')

function configOf(name: string): ts.ParsedCommandLine {
  const path = join(REPO_DIR, name)
  const read = ts.readConfigFile(path, ts.sys.readFile)
  if (read.error) throw new Error(`${name} が読めない: ${ts.flattenDiagnosticMessageText(read.error.messageText, '\n')}`)
  const cfg = ts.parseJsonConfigFileContent(read.config, ts.sys, REPO_DIR, undefined, path)
  expect(cfg.errors, name).toEqual([])
  return cfg
}

/** repo からの道筋 (区切りは /) */
const repoPath = (f: string) => relative(REPO_DIR, f).split(sep).join('/')

/** 読むと Node の型の束を持ち込む物 (@types/node と、それを読む vite・vitest の型) */
const NODE_TYPES = /(^|\/)node_modules\/(@types\/node|vite|vitest)\//

/** 仮のファイル (board の src の中にあるとして解く。手元の盤面には置かない) */
const PROBE = join(REPO_DIR, 'packages', 'board', 'src', '__probe__.ts')

/** 仮のファイルを足した program の、仮のファイルの誤りの番号 */
function probeCodes(cfg: ts.ParsedCommandLine, text: string): number[] {
  const host = ts.createCompilerHost(cfg.options)
  const getSourceFile = host.getSourceFile.bind(host)
  host.getSourceFile = (name, lang, ...rest) =>
    (resolve(name) === PROBE ? ts.createSourceFile(name, text, lang) : getSourceFile(name, lang, ...rest))
  const fileExists = host.fileExists.bind(host)
  host.fileExists = name => resolve(name) === PROBE || fileExists(name)
  const program = ts.createProgram([PROBE], cfg.options, host)
  const sf = program.getSourceFile(PROBE)
  if (!sf) throw new Error('仮のファイルが program に入っていない')
  return [...program.getSyntacticDiagnostics(sf), ...program.getSemanticDiagnostics(sf)].map(d => d.code).sort()
}

/** Node の型が無い時の誤り (`Buffer`・`process` の名前と、`node:fs` のモジュール。どちらも「Do you need to install type definitions for node?」) */
const NO_NODE_TYPES = 2591

describe('ブラウザの入口の型検査 (tsconfig.browser.json)', () => {
  it('設定: types は空・lib は ES2023・DOM・DOM.Iterable。対象は 3 つのパッケージの src で、node.ts と試験は入らない', () => {
    const cfg = configOf('tsconfig.browser.json')
    expect(cfg.options.types).toEqual([])
    expect(cfg.options.lib).toEqual(['lib.es2023.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'])
    const files = cfg.fileNames.map(repoPath)
    for (const dir of ['packages/engine-js/src/', 'packages/board/src/', 'packages/view-3d/src/']) {
      expect(files.some(f => f.startsWith(dir)), dir).toBe(true)
    }
    expect(files).toContain('packages/engine-js/src/index.ts')
    expect(files).toContain('packages/view-3d/src/IsometricView.tsx')
    expect(files).not.toContain('packages/engine-js/src/node.ts')
    expect(files.filter(f => /\.test\.[cm]?tsx?$/.test(f) || !/^packages\/[^/]+\/src\//.test(f))).toEqual([])
  })

  it('npm run typecheck は、ルートの設定とブラウザの設定の両方を回す', () => {
    const pkg = JSON.parse(readFileSync(join(REPO_DIR, 'package.json'), 'utf-8')) as { scripts: Record<string, string> }
    expect(pkg.scripts.typecheck).toContain('tsc -p tsconfig.json')
    expect(pkg.scripts.typecheck).toContain('tsc -p tsconfig.browser.json')
  })

  it('(a) 読むファイルに @types/node (と vite・vitest の型) が無い', () => {
    const cfg = configOf('tsconfig.browser.json')
    const program = ts.createProgram(cfg.fileNames, cfg.options)
    const files = program.getSourceFiles().map(sf => repoPath(sf.fileName))
    expect(files.length).toBeGreaterThan(cfg.fileNames.length)   // lib と依存の型も読んでいる
    expect(files.filter(f => NODE_TYPES.test(f))).toEqual([])
    // 見張りが効くこと: ルートの設定 (types: ["node"]) では @types/node を読む
    const root = configOf('tsconfig.json')
    const rootFiles = ts.createProgram(cfg.fileNames, root.options).getSourceFiles().map(sf => repoPath(sf.fileName))
    expect(rootFiles.some(f => NODE_TYPES.test(f))).toBe(true)
  }, 60_000)

  it('(b) Buffer・process・node:fs を書いた仮のファイルは、ブラウザの設定だけで落ちる', () => {
    const browser = configOf('tsconfig.browser.json')
    const root = configOf('tsconfig.json')
    const probes = {
      Buffer: 'export const a = Buffer.alloc(1)',
      process: 'export const b = process.env.X',
      'node:fs': "import { readFileSync } from 'node:fs'\nexport const c = readFileSync",
    }
    expect(probeCodes(browser, probes.Buffer)).toEqual([NO_NODE_TYPES])
    expect(probeCodes(browser, probes.process)).toEqual([NO_NODE_TYPES])
    expect(probeCodes(browser, probes['node:fs'])).toEqual([NO_NODE_TYPES])
    for (const [name, text] of Object.entries(probes)) expect(probeCodes(root, text), name).toEqual([])
    // 何も Node の物を書かなければ、ブラウザの設定でも通る
    expect(probeCodes(browser, 'export const d: Uint8Array = new TextEncoder().encode("x")')).toEqual([])
  }, 60_000)

  it('ファイルの中の三重スラッシュの指示 (types=node) は types: [] をすり抜ける (だから依存の見張りが拾う)', () => {
    const browser = configOf('tsconfig.browser.json')
    expect(probeCodes(browser, '/// <reference types="node" />\nexport const a = Buffer.alloc(1)')).toEqual([])
  }, 60_000)
})
