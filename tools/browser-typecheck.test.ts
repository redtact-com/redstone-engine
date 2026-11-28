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
 * 落ちること、を見る。
 *
 * ファイルの中の三重スラッシュの指示は、設定の `types` と `lib` では止まらない。`types=node` で @types/node を読み込む形は
 * (a) が読むファイルの一覧で拾い、`lib=esnext` で lib を広げる形は (a) が読む lib の一覧で拾う。
 * board と view-3d は依存の見張りでも拾う。engine-js には依存の見張りが無いので、engine-js で拾うのは (a) だけ。
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

/** 仮のファイル 1 つだけを根にした program */
function probeProgram(cfg: ts.ParsedCommandLine, text: string): { program: ts.Program; sf: ts.SourceFile } {
  const host = ts.createCompilerHost(cfg.options)
  const getSourceFile = host.getSourceFile.bind(host)
  host.getSourceFile = (name, lang, ...rest) =>
    (resolve(name) === PROBE ? ts.createSourceFile(name, text, lang) : getSourceFile(name, lang, ...rest))
  const fileExists = host.fileExists.bind(host)
  host.fileExists = name => resolve(name) === PROBE || fileExists(name)
  const program = ts.createProgram([PROBE], cfg.options, host)
  const sf = program.getSourceFile(PROBE)
  if (!sf) throw new Error('仮のファイルが program に入っていない')
  return { program, sf }
}

/** 仮のファイルを足した program の、仮のファイルの誤りの番号 */
function probeCodes(cfg: ts.ParsedCommandLine, text: string): number[] {
  const { program, sf } = probeProgram(cfg, text)
  return [...program.getSyntacticDiagnostics(sf), ...program.getSemanticDiagnostics(sf)].map(d => d.code).sort()
}

/** program が読んだ TS の lib のファイル (lib.es2023.d.ts など) の名前 */
function libsOf(program: ts.Program): string[] {
  return program.getSourceFiles().map(sf => repoPath(sf.fileName))
    .filter(f => /(^|\/)node_modules\/typescript\/lib\/lib\.[^/]*\.d\.ts$/.test(f))
    .map(f => f.slice(f.lastIndexOf('/') + 1))
    .sort()
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

  it('(a) 読むファイルに @types/node (と vite・vitest の型) が無く、lib は設定の lib から広がらない', () => {
    const cfg = configOf('tsconfig.browser.json')
    const program = ts.createProgram(cfg.fileNames, cfg.options)
    const files = program.getSourceFiles().map(sf => repoPath(sf.fileName))
    expect(files.length).toBeGreaterThan(cfg.fileNames.length)   // lib と依存の型も読んでいる
    expect(files.filter(f => NODE_TYPES.test(f))).toEqual([])
    // lib は、何も書かない仮のファイル 1 つの program と同じ (どこかの `/// <reference lib=… />` で広がっていない)
    const libs = libsOf(probeProgram(cfg, 'export {}').program)
    expect(libs).toContain('lib.es2023.d.ts')
    expect(libs).not.toContain('lib.esnext.d.ts')
    expect(libsOf(program)).toEqual(libs)
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

  it('ファイルの中の三重スラッシュの指示 (types=node・lib=esnext) は設定をすり抜ける (だから (a) が一覧で拾う。engine-js には依存の見張りが無い)', () => {
    const browser = configOf('tsconfig.browser.json')
    expect(probeCodes(browser, '/// <reference types="node" />\nexport const a = Buffer.alloc(1)')).toEqual([])
    // ES2023 に無い API (Set の union) は落ちるが、lib=esnext の指示があると通る
    const union = 'export const u = new Set([1]).union(new Set([2]))'
    expect(probeCodes(browser, union)).toEqual([2550])
    expect(probeCodes(browser, `/// <reference lib="esnext" />\n${union}`)).toEqual([])
    // その時は、読む lib の一覧が広がる ((a) が拾う)
    expect(libsOf(probeProgram(browser, `/// <reference lib="esnext" />\n${union}`).program)).toContain('lib.esnext.d.ts')
  }, 60_000)
})
