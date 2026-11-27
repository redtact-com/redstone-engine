import { describe, it, expect } from 'vitest'
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * 著作権まわりの衛生を見張る (docs/ip-policy.md・redstone-sim の docs/research/19 §9.3)。
 *
 * この repo は挙動を自分の言葉のメモ (redstone-sim の docs/vanilla) から別実装する。
 * 挙動を自分の言葉で書くのは問題にならないが、デコンパイルしたコードの写しは別である。
 * コメント・ドキュメントに Java の字面を貼らない。典拠は「メモの節 + クラス名.メソッド名」で書く。
 *
 * 元は redstone-sim の tools/ip-hygiene.test.ts。Rust と TOML を足し、依存とファイルの種類も見る。
 */

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..')
const SELF = 'tools/ip-hygiene.test.ts'

function tracked(): string[] {
  return execFileSync('git', ['ls-files'], { cwd: repoRoot, encoding: 'utf-8' })
    .split('\n').filter(Boolean)
}

function readText(f: string): string | null {
  try {
    const text = readFileSync(join(repoRoot, f), 'utf-8')
    return text.includes('\0') ? null : text   // バイナリは見ない
  } catch {
    return null
  }
}

/** Java の**文そのもの**に見えるパターン (説明文には出ない形) */
const VERBATIM: { name: string; re: RegExp }[] = [
  { name: 'java コードフェンス', re: /```java/ },
  // 呼び出し文の写し (末尾セミコロン付き)。Rust の文も `;` で終わるので、コメント行だけを見る
  { name: 'Java の呼び出し文', re: /\b(level|state|this|world)\.[A-Za-z]\w*\([^)]*\)\s*;/ },
  // メソッドシグネチャの写し
  { name: 'Java のメソッドシグネチャ', re: /\b(public|protected|private)\s+(static\s+)?[\w<>[\]]+\s+\w+\s*\([^)]*\)\s*\{/ },
  // record 宣言の写し
  { name: 'Java の record 宣言', re: /\brecord\s+[A-Z]\w*(<[^>]*>)?\s*\(/ },
  // 「型 名前」の並ぶ引数リストの写し。Rust と TS は `name: Type` の順なので、Java の字面にしか出ない
  { name: 'Java の型付き引数リスト', re: /\w\s*\(\s*(final\s+)?([A-Z]\w*(<[^>()]*>)?|int|long|boolean|double|float|byte|short|char)(\[\])?\s+[a-z]\w*\s*[,)]/ },
]

/** 拡張子ごとの「コメント行か」。md は全行を見る */
function isCommentLine(f: string, t: string): boolean {
  if (f.endsWith('.md')) return true
  if (f.endsWith('.toml')) return t.startsWith('#')
  return t.startsWith('//') || t.startsWith('*') || t.startsWith('/*')
}

const VERBATIM_EXT = ['.rs', '.toml', '.ts', '.tsx', '.mjs', '.md']
const VERBATIM_SKIP = [SELF, 'docs/ip-policy.md']   // ip-policy は書き方の見本として ✗ 例を載せている

describe('著作権まわりの衛生 (docs/ip-policy.md)', () => {
  it('デコンパイルした Java の逐語引用が入っていない', () => {
    const hits: string[] = []
    for (const f of tracked()) {
      if (!VERBATIM_EXT.some(e => f.endsWith(e)) || VERBATIM_SKIP.includes(f)) continue
      const text = readText(f)
      if (text === null) continue
      text.split('\n').forEach((line, i) => {
        const t = line.trim()
        if (!isCommentLine(f, t)) return
        for (const p of VERBATIM) {
          if (p.re.test(line)) hits.push(`${f}:${i + 1} [${p.name}] ${t.slice(0, 100)}`)
        }
      })
    }
    expect(hits, `Java の字面を貼らないこと (典拠はメモの節 + クラス名.メソッド名 + 自分の言葉で書く)\n${hits.join('\n')}`)
      .toEqual([])
  })

  it('手元の絶対パスがコミットされていない', () => {
    // 公開の repo なので、手元のユーザ名が読める状態を作らない
    const ABSOLUTE: { name: string; re: RegExp }[] = [
      { name: 'エージェントの一時ディレクトリ', re: /\/tmp\/claude-\d+\// },
      { name: 'WSL 経由の Windows パス', re: /\/mnt\/[a-z]\/Users\// },
      { name: 'Windows パス', re: /[A-Za-z]:\\Users\\/ },
      { name: 'ホームディレクトリ', re: /\/(home|Users)\/[A-Za-z0-9._-]+\// },
    ]
    const hits: string[] = []
    for (const f of tracked()) {
      if (f === SELF || f.startsWith('.github/pr-assets/')) continue
      const text = readText(f)
      if (text === null) continue
      text.split('\n').forEach((line, i) => {
        for (const p of ABSOLUTE) {
          if (p.re.test(line)) hits.push(`${f}:${i + 1} [${p.name}] ${line.trim().slice(0, 100)}`)
        }
      })
    }
    expect(hits, `絶対パスをコミットしないこと\n${hits.join('\n')}`).toEqual([])
  })

  it('ゲーム由来のファイル (jar・class・Java のソース・mappings・データの書き出し) を追跡していない', () => {
    const BAD: { name: string; re: RegExp }[] = [
      { name: 'jar', re: /\.jar$/ },
      { name: 'class', re: /\.class$/ },
      { name: 'Java のソース', re: /\.java$/ },
      { name: 'mappings', re: /(\.tiny|\.srg|\.tsrg|\.proguard)$|(^|\/)mappings(\/|\.txt$)/ },
      { name: 'データの書き出し (data report)', re: /(^|\/)(reports|generated)\/(blocks|registries)\.json$|(^|\/)blocks\.json$/ },
      { name: 'デコンパイルの作業場所', re: /(^|\/)decompile\// },
    ]
    const hits = tracked().flatMap(f => BAD.filter(b => b.re.test(f)).map(b => `${f} [${b.name}]`))
    expect(hits, `ゲーム由来のファイルはコミットしない (docs/ip-policy.md §1)\n${hits.join('\n')}`).toEqual([])
  })

  it('テクスチャ・モデル・リソースパック・ゲームのデータの抜き書きを追跡していない (docs/ip-policy.md §2)', () => {
    const hits = assetHits(tracked())
    expect(hits, `アセットは同梱しない。実行時に取得先から取る (docs/ip-policy.md §2)\n${hits.join('\n')}`).toEqual([])
  })

  // 見張りそのものの確かめ: 置き場の形を拾い、自前の絵・PR の画像・取り方のコードは拾わない
  it('アセットの見張りは、パックとアセットの置き場を拾い、ほかの .png とコードは拾わない', () => {
    for (const path of [
      'apps/web/public/resourcepack/assets/minecraft/textures/block/stone.png',
      'apps/web/public/resourcepack/bundle.json',
      'apps/web/public/resource-packs/x/readme.txt',
      'assets/minecraft/models/block/lever.json',
      'packages/view-3d/test/blockstate-variants.1.21.4.json',
      'packages/view-3d/test/blocks_states.json',
      'packages/view-3d/test/blocks_models.json',
      'pack/pack.mcmeta',
      'x/textures/block/lever.png',
      'x/textures/entity/chest/normal.png',
      'x/models/block/lever.json',
      'x/blockstates/lever.json',
    ]) {
      expect(assetHits([path]).length, path).toBeGreaterThan(0)
    }
    expect(assetHits([
      '.github/pr-assets/11-view-3d/view-3d.png',
      'apps/web/public/icon.png',
      'packages/view-3d/src/renderer/texturePaths.ts',
      'packages/view-3d/src/renderer/mcAssets.ts',
      'packages/view-3d/src/renderer/assetSources.ts',
      'docs/assets.md',
    ])).toEqual([])
  })
})

/**
 * テクスチャ・モデル・リソースパックと、ゲームのデータの抜き書きの置き場 (#11)。
 * 取り方 (misode/mcmeta・PrismarineJS/minecraft-assets から実行時に取る) は packages/view-3d の renderer にある。
 * 自前の絵と PR の画像 (.github/pr-assets) があるので、.png の全部は禁じない。
 */
const ASSET_PLACES: { name: string; re: RegExp }[] = [
  { name: 'アセットの置き場 (assets/minecraft/)', re: /(^|\/)assets\/minecraft\// },
  { name: 'リソースパックの置き場', re: /(^|\/)resource-?packs?\//i },
  { name: 'パックの束 (bundle.json)', re: /(^|\/)bundle\.json$/ },
  { name: 'パックの説明 (.mcmeta)', re: /\.mcmeta$/ },
  { name: 'blockstates と models の束 (PrismarineJS)', re: /(^|\/)blocks_(states|models)\.json$/ },
  { name: 'blockstates・models・textures の置き場', re: /(^|\/)(blockstates|models\/(block|item)|textures\/(block|item|entity))\// },
  { name: 'variant の表 (ゲームのデータの抜き書き)', re: /(^|\/)blockstate-variants[^/]*$/ },
]

function assetHits(paths: string[]): string[] {
  return paths.flatMap(f => ASSET_PLACES.filter(p => p.re.test(f)).map(p => `${f} [${p.name}]`))
}

/** unsafe の置き場 (#9) */
const UNSAFE = {
  scratch: 'crates/rse-wasm/src/scratch.rs',
  exports: 'crates/rse-wasm/src/exports.rs',
  wasmRoot: 'crates/rse-wasm/src/lib.rs',
}

/**
 * Rust の文から、注 (行・塊・入れ子)・文字列 (生の文字列も)・文字の字面を空白に置き換える。改行は残すので行の番号は変わらない。
 * unsafe の見張りは、この「文だけ」の字面に当てる (注や文字列の中の unsafe は拾わず、複数行の文は丸ごと見る)。
 */
function rustCode(text: string): string {
  const blank = (t: string) => t.replace(/[^\n]/g, ' ')
  const raw = /b?r(#*)"/y
  const char = /'(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'/y
  let out = ''
  let i = 0
  while (i < text.length) {
    const two = text.slice(i, i + 2)
    let end = -1
    if (two === '//') {
      end = text.indexOf('\n', i)
      if (end < 0) end = text.length
    } else if (two === '/*') {
      let depth = 1
      end = i + 2
      while (end < text.length && depth > 0) {
        const t = text.slice(end, end + 2)
        if (t === '/*') (depth++, (end += 2))
        else if (t === '*/') (depth--, (end += 2))
        else end++
      }
    } else if (!/\w/.test(text[i - 1] ?? '') && ((raw.lastIndex = i), raw.test(text))) {
      raw.lastIndex = i
      const m = raw.exec(text) as RegExpExecArray
      const close = text.indexOf(`"${m[1]}`, i + m[0].length)
      end = close < 0 ? text.length : close + 1 + m[1].length
    } else if (text[i] === '"') {
      end = i + 1
      while (end < text.length && text[end] !== '"') end += text[end] === '\\' ? 2 : 1
      end++
    } else if (text[i] === "'" && ((char.lastIndex = i), char.test(text))) {
      end = char.lastIndex
    }
    if (end > i) {
      out += blank(text.slice(i, end))
      i = end
    } else {
      out += text[i]
      i++
    }
  }
  return out
}

/** rustdoc が Rust として組む囲みの言葉 (`,` か空白で区切った全部がこれなら doctest) */
const RUSTDOC_WORD = /^(rust|ignore|should_panic|no_run|compile_fail|standalone_crate|test_harness|edition\d{4}|ignore-[\w-]+|E\d{4})$/

/**
 * 注の中の doctest の行を返す。`///` と `//!` の続きを 1 つの塊として、囲み (``` か ~~~ を 3 つ以上。言葉が無いか、
 * 言葉が全部 rustdoc の印) と、空の行の後の 4 字以上の字下げのコードを拾う。#[doc = "…"] の形は拾わない。
 */
function doctestLines(text: string): number[] {
  const out: number[] = []
  let fence: { mark: string; size: number } | null = null
  let prevBlank = true
  text.split('\n').forEach((line, i) => {
    const doc = /^\s*\/\/[/!](?![/!])(.*)$/.exec(line)
    if (!doc) {
      fence = null
      prevBlank = true
      return
    }
    const body = doc[1].replace(/^ /, '')
    const open = /^ {0,3}(`{3,}|~{3,})\s*(.*)$/.exec(body)
    if (fence) {
      const f = fence as { mark: string; size: number }
      if (open && open[1][0] === f.mark && open[1].length >= f.size && open[2] === '') fence = null
      return
    }
    if (open) {
      const words = open[2].split(/[\s,]+/).filter(Boolean)
      if (words.every(w => RUSTDOC_WORD.test(w))) out.push(i + 1)
      fence = { mark: open[1][0], size: open[1].length }
      prevBlank = false
      return
    }
    if (prevBlank && /^( {4}|\t)\s*\S/.test(body)) out.push(i + 1)
    prevBlank = body.trim() === ''
  })
  return out
}

/** crates の .rs の、unsafe の置き場の決まりに外れた所 */
function unsafeHits(files: { path: string; text: string }[]): string[] {
  const hits: string[] = []
  for (const { path, text } of files) {
    const code = rustCode(text)
    const lines = code.split('\n')
    if (/^crates\/[^/]+\/src\/(lib|main)\.rs$/.test(path)) {
      if (path === UNSAFE.wasmRoot) {
        if (!/^#!\[deny\(unsafe_code\)\]\s*$/m.test(code)) hits.push(`${path}: #![deny(unsafe_code)] が無い`)
      } else if (!path.startsWith('crates/rse-wasm/') && !/^#!\[forbid\(unsafe_code\)\]\s*$/m.test(code)) {
        hits.push(`${path}: #![forbid(unsafe_code)] が無い`)
      }
    }
    lines.forEach((line, i) => {
      for (const m of line.matchAll(/\bunsafe\b/g)) {
        const attr = /^\s*\(/.test(line.slice((m.index ?? 0) + m[0].length))
        // scratch.rs は unsafe のブロックの置き場。exports.rs は unsafe の属性 (#[unsafe(no_mangle)]) だけ
        if (path === UNSAFE.scratch || (path === UNSAFE.exports && attr)) continue
        hits.push(`${path}:${i + 1} unsafe: ${line.trim()}`)
      }
    })
    if (path !== UNSAFE.scratch && path !== UNSAFE.exports) {
      // 文の全体に当てる (rustfmt が折った複数行の属性も拾う)
      for (const m of code.matchAll(/\b(allow|expect|warn)\s*\(([^()]*)\)/g)) {
        if (/\bunsafe_code\b/.test(m[2])) {
          const at = code.slice(0, m.index).split('\n').length
          hits.push(`${path}:${at} unsafe_code を許している: ${m[0].replace(/\s+/g, ' ')}`)
        }
      }
    }
    for (const at of doctestLines(text)) {
      hits.push(`${path}:${at} doctest がある (forbid が効かないので、先に #![doc(test(attr(forbid(unsafe_code))))] を付ける)`)
    }
  }
  return hits
}

describe('Rust の依存の見張り (19 §9.3)', () => {
  /** rse-* 以外で使ってよい外部の crate。足す時は PR で理由を書く */
  const ALLOWED_EXTERNAL: string[] = [
    // rse-cli が fixture の JSON を読むのに使う (#5)。エンジンの crate (rse-cli 以外) には入れない。
    // 下のテスト「エンジンの crate は外部の依存を持たない」が、それを見張る
    'serde_json',
    // serde_json がビルドで引く依存
    'serde_core', 'itoa', 'memchr', 'zmij',
    // ビルドはされないが、Cargo.lock に名前が出る物。serde と serde_derive は serde_json と serde_core が
    // 版を揃えるために挙げていて、残りは serde_derive の依存である。derive は使っていない
    'serde', 'serde_derive', 'proc-macro2', 'quote', 'syn', 'unicode-ident',
  ]
  /** 依存にも写しにもしない物 (redstone-sim の docs/research/16 §5) */
  const FORBIDDEN = /nucleation|mc-?tick|redsim/i

  it('Cargo.lock に許可していない crate が無い', () => {
    const lock = readText('Cargo.lock') ?? ''
    const names = [...lock.matchAll(/^name = "([^"]+)"$/gm)].map(m => m[1])
    expect(names.length, 'Cargo.lock を追跡すること').toBeGreaterThan(0)
    const bad = names.filter(n => FORBIDDEN.test(n) || !(n.startsWith('rse-') || ALLOWED_EXTERNAL.includes(n)))
    expect(bad, `許可リスト (tools/ip-hygiene.test.ts の ALLOWED_EXTERNAL) に無い crate\n${bad.join('\n')}`).toEqual([])
  })

  it('エンジンの crate は外部の依存を持たない (path の依存だけ)', () => {
    // serde などの外部の依存は rse-cli だけで使う
    const ENGINE = tracked().filter(f => /^crates\/rse-(?!cli\/)[^/]+\/Cargo\.toml$/.test(f))
    const hits: string[] = []
    for (const f of ENGINE) {
      const text = readText(f) ?? ''
      const deps = text.split(/^\[/m).filter(s => /^(dev-|build-)?dependencies\]/.test(s))
      for (const sec of deps) {
        for (const line of sec.split('\n').slice(1)) {
          const t = line.trim()
          if (t === '' || t.startsWith('#')) continue
          if (!/path\s*=/.test(t)) hits.push(`${f}: ${t}`)
        }
      }
    }
    expect(hits, `エンジンの crate に外部の依存を入れない\n${hits.join('\n')}`).toEqual([])
  })

  // CLAUDE.md の「unsafe は rse-wasm だけ」。rse-wasm の中でも、unsafe のブロックは作業領域の読み書き
  // (scratch.rs) だけに置き、unsafe の属性 (no_mangle) は書き出しの関数 (exports.rs) だけに置く (#9)。
  // unsafe_code を許す (allow・expect・warn) のもこの 2 つだけで、rse-wasm の lib.rs は crate 全体で deny する。
  // doctest の中の unsafe は crate の forbid が効かないので、doctest があれば落とす (doc(test(attr(…))) を付けるまで)
  it('unsafe は rse-wasm の決めたファイルだけで、ほかの crate は forbid(unsafe_code)', () => {
    const files = tracked()
      .filter(f => f.startsWith('crates/') && f.endsWith('.rs'))
      .map(path => ({ path, text: readText(path) ?? '' }))
    const hits = unsafeHits(files)
    expect(hits, `unsafe の置き場は ${UNSAFE.scratch} (ブロック) と ${UNSAFE.exports} (no_mangle) だけ\n${hits.join('\n')}`).toEqual([])
  })

  // 見張りそのものの確かめ: すり抜けた形 (2 回目の見直し) を拾い、注と文字列の中は拾わない
  it('unsafe の見張りは、複数行の allow・macro の引数・cfg_attr・いろいろな doctest の囲みを拾う', () => {
    const other = 'crates/rse-core/src/x.rs'
    const hitsOf = (path: string, text: string) => unsafeHits([{ path, text }])
    // 拾う
    expect(hitsOf(other, '#![allow(\n    dead_code,\n    unsafe_code\n)]\n')).toHaveLength(1)
    expect(hitsOf(other, 'm!(unsafe);\n')).toHaveLength(1)
    expect(hitsOf(other, '#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]\n')).toHaveLength(1)
    expect(hitsOf(UNSAFE.exports, 'fn f() { unsafe { g() } }\n')).toHaveLength(1)
    for (const doc of [
      '/// ```\n/// x\n/// ```\n',
      '/// ```rust,no_run\n/// x\n/// ```\n',
      '//! ```edition2024\n//! x\n//! ```\n',
      '/// ````\n/// x\n/// ````\n',
      '/// ~~~\n/// x\n/// ~~~\n',
      '/// 説明。\n///\n///     let x = 1;\n',
    ]) {
      expect(hitsOf(other, doc), doc).toHaveLength(1)
    }
    // 拾わない
    expect(hitsOf(other, '// unsafe { x }\n/* unsafe { */\nlet s = "unsafe { }"; let r = r#"unsafe(x)"#;\n')).toEqual([])
    expect(hitsOf(other, '/// ```text\n/// unsafe { }\n/// ```\n')).toEqual([])
    // 文字の字面の " は文字列を開かない (開くと、後ろの unsafe を読み飛ばす)。lifetime の ' も同じ
    expect(hitsOf(other, "fn f<'a>(x: &'a u8) -> char { '\"' } m!(unsafe);\n")).toHaveLength(1)
    expect(hitsOf(UNSAFE.scratch, 'fn f() { unsafe { g() } }\n#![allow(unsafe_code)]\n')).toEqual([])
    expect(hitsOf(UNSAFE.exports, '#![allow(unsafe_code)]\n#[unsafe(no_mangle)]\npub extern "C" fn f() {}\n')).toEqual([])
    // 置き場の決まり: ほかの crate の lib.rs には forbid、rse-wasm の lib.rs には deny が要る
    expect(hitsOf('crates/rse-core/src/lib.rs', '// #![forbid(unsafe_code)]\n')).toHaveLength(1)
    expect(hitsOf('crates/rse-core/src/lib.rs', '#![forbid(unsafe_code)]\n')).toEqual([])
    expect(hitsOf(UNSAFE.wasmRoot, '#![allow(unsafe_code)]\n')).toHaveLength(2)
  })

  it('include_bytes / include_str は決めたフォルダ (crates/*/data・conformance) の物だけ読む', () => {
    const hits: string[] = []
    for (const f of tracked().filter(f => f.endsWith('.rs'))) {
      const text = readText(f) ?? ''
      for (const m of text.matchAll(/include_(bytes|str)!\(\s*"([^"]+)"/g)) {
        if (!/(^|\/)(data|conformance)\//.test(m[2])) hits.push(`${f}: ${m[0]}`)
      }
    }
    expect(hits, `データの持ち込みは決めたフォルダだけ\n${hits.join('\n')}`).toEqual([])
  })
})
