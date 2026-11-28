import { describe, it, expect } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

/**
 * board の依存と書き出しの見張り (docs/architecture.md §1: board の依存は無し)。
 * 見張りの形は view-3d の board-to-structure.test.ts と同じ (構文木でモジュールを指す所を集める)。
 * board は view-3d に依らないので、関数は分けて持つ。
 */

const BOARD_DIR = fileURLToPath(new URL('..', import.meta.url))   // packages/board/

/** 見張る TS のファイル (.ts・.tsx・.mts・.cts。.d.ts も含む) */
const SOURCE_FILE = /\.[cm]?tsx?$/
/** 試験のファイル */
const TEST_FILE = /\.test\.[cm]?tsx?$/

/** 置き場 (`root` からの `dir`) の下の、見張る TS のファイル (`dir` からの道筋)。`root` は試験で一時のフォルダを渡す */
function sourcesOf(dir: string, root = BOARD_DIR): string[] {
  return (readdirSync(join(root, dir), { recursive: true }) as string[])
    .filter(f => SOURCE_FILE.test(f))
    .map(f => join(dir, f))
}

interface Specifier {
  readonly spec: string
  /**
   * `module`: import などの指定子・`types`: `/// <reference types=… />`・`path`: `/// <reference path=… />`・
   * `lib`: `/// <reference lib=… />`
   */
  readonly kind: 'module' | 'types' | 'path' | 'lib'
  readonly line: number
}

/**
 * ファイルの中の、モジュールを指す所を全部集める (引用符の種類・改行・注を問わない)。
 * import・export … from・import type・`import(…)`・`require(…)`・型の中の `import('…')`。指定子が文字列でなければ `<式>`。
 * 三重スラッシュの指示 (`types=`・`path=`・`lib=`) も集める。設定の `types: []` と `lib` は、ファイルの中の指示を止めない
 */
function specifiersOf(file: string, text: string): Specifier[] {
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const out: Specifier[] = []
  const at = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1
  const literal = (n: ts.Node | undefined) => (n && ts.isStringLiteralLike(n) ? n.text : '<式>')
  const visit = (n: ts.Node): void => {
    if (ts.isImportDeclaration(n)) {
      out.push({ spec: literal(n.moduleSpecifier), kind: 'module', line: at(n) })
    } else if (ts.isExportDeclaration(n) && n.moduleSpecifier) {
      out.push({ spec: literal(n.moduleSpecifier), kind: 'module', line: at(n) })
    } else if (ts.isImportEqualsDeclaration(n) && ts.isExternalModuleReference(n.moduleReference)) {
      out.push({ spec: literal(n.moduleReference.expression), kind: 'module', line: at(n) })
    } else if (ts.isCallExpression(n)
      && (n.expression.kind === ts.SyntaxKind.ImportKeyword || (ts.isIdentifier(n.expression) && n.expression.text === 'require'))) {
      out.push({ spec: literal(n.arguments[0]), kind: 'module', line: at(n) })
    } else if (ts.isImportTypeNode(n) && ts.isLiteralTypeNode(n.argument)) {
      out.push({ spec: literal(n.argument.literal), kind: 'module', line: at(n) })
    }
    ts.forEachChild(n, visit)
  }
  visit(sf)
  const lineOf = (pos: number) => sf.getLineAndCharacterOfPosition(pos).line + 1
  for (const d of sf.typeReferenceDirectives) out.push({ spec: d.fileName, kind: 'types', line: lineOf(d.pos) })
  for (const d of sf.referencedFiles) out.push({ spec: d.fileName, kind: 'path', line: lineOf(d.pos) })
  for (const d of sf.libReferenceDirectives) out.push({ spec: d.fileName, kind: 'lib', line: lineOf(d.pos) })
  return out
}

/** パッケージの名前 (`@scope/name` か、最初の区切りまで) */
function packageOf(spec: string): string {
  const parts = spec.split('/')
  return spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0]
}

/** 試験が使ってよいパッケージ (下のパスも含む。src は相対の import だけ) */
const TEST_PACKAGES = new Set(['vitest', 'typescript'])

/** 依存の決まりに外れた所 (file は packages/board/ からの道筋) */
function dependencyHits(file: string, text: string): string[] {
  const isTest = TEST_FILE.test(file) || file.startsWith(`test${sep}`) || file.startsWith('test/')
  const hits: string[] = []
  // JSX は react/jsx-runtime を暗に読む (指定子として現れない) ので、src に .tsx を置かない
  if (!isTest && file.endsWith('.tsx')) hits.push(`${file} (src に .tsx を置かない。JSX は react/jsx-runtime を暗に読む)`)
  const outside = (spec: string) => relative(BOARD_DIR, resolve(BOARD_DIR, dirname(file), spec)).startsWith('..')
  for (const { spec, kind, line } of specifiersOf(file, text)) {
    const where = `${file}:${line} ${kind === 'module' ? spec : `/// ${kind}=${spec}`}`
    if (kind === 'path') {
      // path= は、先頭が . でなくてもファイルからの相対パスとして読まれる
      if (outside(spec)) hits.push(`${where} (board の外を指す path=)`)
    } else if (kind === 'lib') {
      // lib に Node の型は無いが、ブラウザの設定の lib (ES2023・DOM・DOM.Iterable) を広げてしまう
      if (!isTest) hits.push(`${where} (src に lib= を書かない。ブラウザの設定の lib を広げる)`)
    } else if (kind === 'types') {
      if (!isTest) hits.push(`${where} (src に types= を書かない。型の束を読み込む)`)
      else if (spec !== 'node' && !TEST_PACKAGES.has(packageOf(spec))) hits.push(`${where} (許していない types=)`)
    } else if (spec === '<式>') {
      hits.push(`${where} (指定子が文字列でない。確かめられない)`)
    } else if (spec.startsWith('.')) {
      if (outside(spec)) hits.push(`${where} (board の外を指す相対パス)`)
    } else if (spec.startsWith('node:')) {
      if (!isTest) hits.push(`${where} (src に Node の物を入れない)`)
    } else if (!(isTest && TEST_PACKAGES.has(packageOf(spec)))) {
      hits.push(`${where} (board はパッケージに依らない)`)
    }
  }
  return hits
}

/** index.ts が書き出す名前 (`export *` は名前が決まらないので、数を別に返す) */
function exportedNamesOf(text: string): { names: string[]; star: number } {
  const sf = ts.createSourceFile('index.ts', text, ts.ScriptTarget.Latest, true)
  const names: string[] = []
  let star = 0
  for (const st of sf.statements) {
    if (ts.isExportDeclaration(st)) {
      if (!st.exportClause || ts.isNamespaceExport(st.exportClause)) star++
      else for (const e of st.exportClause.elements) names.push(e.name.text)
    } else if (ts.isExportAssignment(st)) {
      names.push(st.isExportEquals ? 'export=' : 'default')
    } else if (ts.canHaveModifiers(st) && ts.getModifiers(st)?.some(m => m.kind === ts.SyntaxKind.ExportKeyword)) {
      const isDefault = ts.getModifiers(st)?.some(m => m.kind === ts.SyntaxKind.DefaultKeyword)
      if (isDefault) names.push('default')
      else if (ts.isVariableStatement(st)) for (const d of st.declarationList.declarations) names.push(d.name.getText(sf))
      else names.push((st as ts.DeclarationStatement).name?.getText(sf) ?? '<名前なし>')
    }
  }
  return { names: names.sort(), star }
}

/** 窓口として書き出す名前 (足す・外す時はここも直す) */
const PUBLIC_NAMES = [
  // 型 (view-3d から移した 6 つと、道具)
  'Board', 'BoardBounds', 'CellChange', 'PickRole', 'PickRoleFn', 'PlaceTool', 'Pos3', 'Tool',
  // 範囲・床・道具
  'BOARD_BOUNDS', 'EDIT_Y', 'FLOOR_STATE', 'FLOOR_Y', 'PLACE_STATES', 'TOOLS',
  // 盤面の操作と判定
  'applyChanges', 'blockNamesOf', 'cellKey', 'cellsToLoad', 'createBoard', 'isAllOff', 'isEditable', 'isSameBlock',
].sort()

describe('board の依存と書き出し (docs/architecture.md §1)', () => {
  const files = [...sourcesOf('src'), ...sourcesOf('test')].map(f => ({ f, text: readFileSync(join(BOARD_DIR, f), 'utf-8') }))

  it('依存は無し: src は board の中の相対の import だけ。試験は vitest・typescript・node: の物まで', () => {
    expect(files.filter(x => x.f.startsWith(`src${sep}`)).length).toBeGreaterThanOrEqual(4)
    expect(files.flatMap(({ f, text }) => dependencyHits(f, text))).toEqual([])
  })

  it('package.json に依存の欄が無い', () => {
    const pkg = JSON.parse(readFileSync(join(BOARD_DIR, 'package.json'), 'utf-8')) as Record<string, unknown>
    for (const k of ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies', 'bundleDependencies']) {
      expect(pkg[k], k).toBeUndefined()
    }
    expect(pkg.exports).toEqual({ '.': './src/index.ts' })
  })

  it('見張りは、パッケージ・node:・board の外への相対パス・動的 import・require・型の import を拾う', () => {
    // 見張りの試験の字面が、この試験ファイル自身を見張りに引っかけないよう、IMPORT を置き換えて作る
    const src = (t: string) => t.replace(/IMPORT/g, 'import')
    const hits = (t: string, file = join('src', 'x.ts')) => dependencyHits(file, src(t))
    expect(hits("IMPORT { x } from './board'")).toEqual([])
    expect(hits("IMPORT type { Board } from './types'")).toEqual([])
    expect(hits('IMPORT { Engine } from "@redstone-engine/engine-js"')).toHaveLength(1)
    expect(hits("IMPORT type { Board } from '@redstone-engine/view-3d'")).toHaveLength(1)
    expect(hits("IMPORT { x } from '../../view-3d/src/types'")).toHaveLength(1)
    expect(hits("IMPORT { x } from '../src/board'", join('test', 'a.test.ts'))).toEqual([])
    expect(hits("IMPORT { x } from '../../engine-js/src/index'", join('test', 'a.test.ts'))).toHaveLength(1)
    expect(hits("IMPORT { readFileSync } from 'node:fs'")).toHaveLength(1)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('test', 'a.test.ts'))).toEqual([])
    expect(hits("IMPORT { describe } from 'vitest'")).toHaveLength(1)
    expect(hits("const m = await IMPORT('@redstone-engine/engine-js')")).toHaveLength(1)
    expect(hits('const m = await IMPORT(name)')).toHaveLength(1)
    expect(hits("export * from 'react'")).toHaveLength(1)
    expect(hits("const m = require('node:fs')")).toHaveLength(1)
    expect(hits("let t: IMPORT('@redstone-engine/view-3d').Board")).toHaveLength(1)
    // 試験の使ってよいパッケージは、下のパスも通す
    expect(hits("IMPORT { defineConfig } from 'vitest/config'", join('test', 'a.test.ts'))).toEqual([])
    expect(hits("IMPORT { defineConfig } from 'vite'", join('test', 'a.test.ts'))).toHaveLength(1)
    // 注の中の字面は拾わない
    expect(hits("// IMPORT { Engine } from '@redstone-engine/engine-js'\nconst a = 1")).toEqual([])
  })

  it('見張りは、三重スラッシュの指示 (types=・path=)・.mts と .cts・src の .tsx も拾う', () => {
    const hits = (t: string, file = join('src', 'x.ts')) => dependencyHits(file, t.replace(/IMPORT/g, 'import'))
    const ref = (attr: string) => `/// <reference ${attr} />\nexport const a = 1`
    // types= は src では全部外れ (ブラウザの型検査の types: [] をすり抜けて @types/node などを読み込む)
    expect(hits(ref('types="node"'))).toHaveLength(1)
    expect(hits(ref("types='vite/client'"))).toHaveLength(1)
    expect(hits(ref('types="node"'), join('test', 'a.test.ts'))).toEqual([])
    expect(hits(ref('types="vite/client"'), join('test', 'a.test.ts'))).toHaveLength(1)
    // path= は board の外を指す物が外れ (先頭が . でなくても相対)
    expect(hits(ref('path="./types.ts"'))).toEqual([])
    expect(hits(ref('path="types.ts"'))).toEqual([])
    expect(hits(ref('path="../../engine-js/src/node.ts"'))).toHaveLength(1)
    expect(hits(ref('path="sub/../../../engine-js/src/node.ts"'))).toHaveLength(1)   // 先頭が . でなくても外へ出る
    expect(hits(ref('path="../../../node_modules/@types/node/index.d.ts"'), join('test', 'a.test.ts'))).toHaveLength(1)
    // lib= は src では外れ (ブラウザの設定の lib を広げる)。試験は通す
    expect(hits(ref('lib="esnext"'))).toHaveLength(1)
    expect(hits(ref('lib="dom"'))).toHaveLength(1)
    expect(hits(ref('lib="esnext"'), join('test', 'a.test.ts'))).toEqual([])
    // .mts・.cts も読み、試験のファイルの見分けも同じ
    for (const f of ['a.ts', 'a.tsx', 'a.mts', 'a.cts', 'a.d.ts', 'a.d.mts']) expect(SOURCE_FILE.test(f), f).toBe(true)
    for (const f of ['a.js', 'a.mjs', 'a.json', 'a.ts.bak']) expect(SOURCE_FILE.test(f), f).toBe(false)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('src', 'x.mts'))).toHaveLength(1)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('src', 'x.cts'))).toHaveLength(1)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('src', 'x.test.mts'))).toEqual([])
    // src の .tsx は中身によらず外れ (JSX が react/jsx-runtime を暗に読む)。試験の .tsx は通す
    expect(hits('export const a = 1', join('src', 'x.tsx'))).toHaveLength(1)
    expect(hits('export const a = 1', join('test', 'a.test.tsx'))).toEqual([])
  })

  it('見張るファイルの一覧は .ts・.tsx・.mts・.cts と型の宣言を拾い、.js は拾わない (一時のフォルダで確かめる)', () => {
    const root = mkdtempSync(join(tmpdir(), 'board-guard-'))
    try {
      mkdirSync(join(root, 'src', 'sub'), { recursive: true })
      const files: Record<string, string> = {
        'a.ts': 'export const a = 1',
        'b.mts': "IMPORT { readFileSync } from 'node:fs'\nexport const b = readFileSync",
        'c.cts': 'export const c = 1',
        'd.js': "IMPORT { readFileSync } from 'node:fs'",
        'e.d.mts': 'export declare const e: number',
        'sub/f.tsx': 'export const f = 1',
      }
      for (const [name, text] of Object.entries(files)) writeFileSync(join(root, 'src', name), text.replace(/IMPORT/g, 'import'))
      const listed = sourcesOf('src', root)
      expect([...listed].sort()).toEqual(['a.ts', 'b.mts', 'c.cts', 'e.d.mts', join('sub', 'f.tsx')].map(f => join('src', f)).sort())
      // 一覧に入った .mts の node: の import と、src の .tsx を、依存の見張りが拾う
      const hits = listed.flatMap(f => dependencyHits(f, readFileSync(join(root, f), 'utf-8')))
      expect(hits.some(h => h.startsWith(`${join('src', 'b.mts')}:1 node:fs`)), hits.join(' / ')).toBe(true)
      expect(hits.some(h => h.startsWith(join('src', 'sub', 'f.tsx'))), hits.join(' / ')).toBe(true)
      expect(hits).toHaveLength(2)
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  it('index.ts は export * を使わず、書き出す名前は決めた一覧のとおり', () => {
    const index = files.find(x => x.f === join('src', 'index.ts'))?.text ?? ''
    const { names, star } = exportedNamesOf(index)
    expect(star).toBe(0)
    expect(names).toEqual(PUBLIC_NAMES)
    // 見張りが効くこと
    expect(exportedNamesOf("export * from './x'\nexport * as y from './y'").star).toBe(2)
    expect(exportedNamesOf('export default 42').names).toEqual(['default'])
    expect(exportedNamesOf('export const a = 1, b = 2\nexport function c() {}').names).toEqual(['a', 'b', 'c'])
  })
})
