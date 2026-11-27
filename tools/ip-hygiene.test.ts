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
})

describe('Rust の依存の見張り (19 §9.3)', () => {
  /** rse-* 以外で使ってよい外部の crate。足す時は PR で理由を書く */
  const ALLOWED_EXTERNAL: string[] = []
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
