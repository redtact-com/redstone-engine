/**
 * 描く盤面 (BoardModel): 範囲と「`"x,y,z"` → 正規化した blockstate の文字列」の表と、
 * それを deepslate に見せる自前の StructureProvider。
 *
 * React と WebGL の外の、純粋なクラスにしてある。盤面の差し替え (`setBoard`)・変化の列 (`applyChanges`)・
 * 読み込み中に来た変化を全部ここへ当て、描く側 (IsometricView) は返ってきた「何を作り直すか」に従うだけにする。
 * 構造・資源の名前・ピック・blockToClient・stateAt も、ここから引く。
 *
 * 移す元は redstone-sim の packages/viewer/src/world-to-structure.ts (develop a40ae23)。
 * sim の BlockState を文字列に組み直す所 (blockStateToMinecraftStr・flipDir ほか) と、
 * 固定のプリロードの一覧 (51 名) は外した。入力がもう文字列なので要らない。
 * deepslate の Structure の中身を型を外して書き換えるのもやめた (下の CellStructure)。
 */

import { BlockState } from 'deepslate/core'
import type { BlockPos, StructureProvider } from 'deepslate/core'
import type { ApplyResult, Board, BoardBounds, CellChange, Pos3 } from './types'

// ── 文字列の正規化 ────────────────────────────────────────────────────────

/** 何も無いマスとして扱う名前 */
const EMPTY_NAMES: ReadonlySet<string> = new Set(['air', 'cave_air', 'void_air'])

const NAMESPACE = 'minecraft:'

/** 正規化の結果を覚える (盤面の差し替えのたびに同じ文字列を何千回も直さない)。数が増えたら捨てる */
const normalized = new Map<string, string | null>()
const NORMALIZED_LIMIT = 65536

/**
 * blockstate の文字列を、描く盤面の形にそろえる。何も無いマス (air・cave_air・void_air) は null。
 *
 * - 先頭の `minecraft:` を外す (ほかの名前空間はそのまま)
 * - **プロパティは名前順に並べ直す**。engine-js の `StateTable.name` と同じ並びなので、
 *   その文字列はそのまま通る。並びの違う文字列 (`lever[powered=true,face=floor,...]`) も同じマスとして比べられる
 * - 空の `[]` は外す。`[` で始まって `]` で閉じない崩れた形は、そのまま 1 つの名前として扱う (描けずに消える)
 */
export function normalizeState(state: string): string | null {
  if (typeof state !== 'string') throw new TypeError(`blockstate が文字列でない: ${String(state)}`)
  const hit = normalized.get(state)
  if (hit !== undefined) return hit
  const out = normalize(state)
  if (normalized.size >= NORMALIZED_LIMIT) normalized.clear()
  normalized.set(state, out)
  return out
}

function normalize(raw: string): string | null {
  const s = raw.startsWith(NAMESPACE) ? raw.slice(NAMESPACE.length) : raw
  const open = s.indexOf('[')
  const name = open < 0 ? s : s.slice(0, open)
  if (EMPTY_NAMES.has(name)) return null
  if (open < 0 || !s.endsWith(']')) return s
  const body = s.slice(open + 1, -1)
  if (body === '') return name
  const pairs = body.split(',').map(kv => {
    const eq = kv.indexOf('=')
    return { key: eq < 0 ? kv : kv.slice(0, eq), kv }
  })
  // 鍵で比べる (「k=v」の文字列のままだと、鍵が別の鍵の頭になる時に `=` と数字の大小で崩れる)
  pairs.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0))
  return `${name}[${pairs.map(p => p.kv).join(',')}]`
}

/** 文字列のブロックの名前 (`[` より前)。正規化した文字列に使う */
export function blockNameOf(state: string): string {
  const open = state.indexOf('[')
  return open < 0 ? state : state.slice(0, open)
}

/** 盤面の鍵 `"x,y,z"` */
export function cellKey(x: number, y: number, z: number): string {
  return `${x},${y},${z}`
}

/** 鍵の 1 つの部分 (符号と数字だけ。空白・`+`・小数点・指数は崩れた形) */
const KEY_PART = /^-?\d+$/

/** `"x,y,z"` を座標にする。3 つの部分がどれも整数の字面 (`/^-?\d+$/`) でなければ null */
function parseKey(key: string): Pos3 | null {
  const parts = key.split(',')
  if (parts.length !== 3 || !parts.every(v => KEY_PART.test(v))) return null
  const p = parts.map(Number) as Pos3
  return p.every(Number.isSafeInteger) ? p : null
}

// ── 描く名前 ──────────────────────────────────────────────────────────────

/**
 * deepslate が描けない名前の読み替え (redstone-sim#343)。
 *
 * シュルカーボックスはブロックのモデルを持たず、deepslate の SpecialRenderers が名前で引いて描く。
 * その表は**色付きの 16 色しか持たない** (`{color}_shulker_box`) ので、無染色の `shulker_box` は
 * 何も描かれずに消える。アイテムの見た目に一番近い紫へ寄せる。盤面の状態 (stateAt) は読み替えない。
 */
const DRAW_ALIAS: Readonly<Record<string, string>> = {
  shulker_box: 'purple_shulker_box',
}

/** 描く時のブロックの名前 (読み替えた後) */
export function drawNameOf(name: string): string {
  return DRAW_ALIAS[name] ?? name
}

/**
 * 先に資源を作る名前にそろえる (`preloadNames` と盤面に出た名前に使う)。
 * `minecraft:` とプロパティを外し、読み替えを当てる。何も無いマスの名前は null。
 */
export function resourceNameOf(state: string): string | null {
  const n = normalizeState(state)
  return n === null ? null : drawNameOf(blockNameOf(n))
}

/** 正規化した文字列から、描く deepslate の状態を作る */
function drawStateOf(state: string): BlockState {
  const open = state.indexOf('[')
  if (open < 0 || !state.endsWith(']')) return new BlockState(drawNameOf(state))
  const props: Record<string, string> = {}
  for (const kv of state.slice(open + 1, -1).split(',')) {
    const eq = kv.indexOf('=')
    if (eq > 0) props[kv.slice(0, eq)] = kv.slice(eq + 1)
  }
  return new BlockState(drawNameOf(state.slice(0, open)), props)
}

// ── 構造 (deepslate の StructureProvider) ────────────────────────────────

/** deepslate の ChunkBuilder の既定のチャンクの大きさ (StructureRenderer の chunkSize を渡していない) */
export const CHUNK_SIZE = 16

/**
 * 違うチャンクがこれより多ければ、チャンクを選ばずに全部を作り直す。
 *
 * deepslate の `updateStructureBuffers(チャンクの一覧)` は、一覧を渡しても**全部のブロックを回り**、
 * ブロックごとに一覧を前から比べる (ブロックの数 × 一覧の長さ)。メッシュを作るのは一覧のチャンクだけ。
 * 一覧が長いと比べる手間が増え、チャンクの大半なら全部を作るのと変わらないので、
 * 「64 個を超える」か「全部のチャンクの半分を超える」と全部を作り直す (`{ kind: 'full', resized: false }`)。
 * UNVERIFIED: どちらの値も、手元の計測ではなく見積もり (大きい盤面で updateStructureBuffers の時間を測って決め直す)。
 */
export const MAX_CHUNK_LIST = 64

interface Placed {
  readonly pos: BlockPos
  readonly state: BlockState
}

/**
 * 盤面を deepslate に見せる StructureProvider。座標は構造ローカル (範囲の最小が原点)。
 *
 * - `getBlock` は範囲の外と空のマスで null を返す (ChunkBuilder の面の隠し (needsCull) が隣を引く)
 * - `getBlocks` の配列は、変わるまで同じ物を返す (ChunkBuilder は作り直すたびに全部を回る)
 */
class CellStructure implements StructureProvider {
  readonly #size: BlockPos
  readonly #cells = new Map<number, Placed>()
  #list: Placed[] | null = null

  constructor(size: BlockPos) {
    this.#size = size
  }

  #index(x: number, y: number, z: number): number {
    return (x * this.#size[1] + y) * this.#size[2] + z
  }

  inside(x: number, y: number, z: number): boolean {
    const s = this.#size
    return x >= 0 && x < s[0] && y >= 0 && y < s[1] && z >= 0 && z < s[2]
  }

  /** null で消す */
  set(pos: BlockPos, state: BlockState | null): void {
    const i = this.#index(pos[0], pos[1], pos[2])
    if (state === null) this.#cells.delete(i)
    else this.#cells.set(i, { pos, state })
    this.#list = null
  }

  getSize(): BlockPos {
    return this.#size
  }

  getBlocks(): Placed[] {
    this.#list ??= [...this.#cells.values()]
    return this.#list
  }

  getBlock(pos: BlockPos): Placed | null {
    if (!this.inside(pos[0], pos[1], pos[2])) return null
    return this.#cells.get(this.#index(pos[0], pos[1], pos[2])) ?? null
  }
}

// ── 描く盤面 ──────────────────────────────────────────────────────────────

/** 何を作り直すか */
export type Redraw =
  | { readonly kind: 'none' }
  /** 全部を作り直す。`resized` なら範囲が変わったので、setStructure (グリッドも作り直す) と寄りの合わせ直し */
  | { readonly kind: 'full'; readonly resized: boolean }
  /** このチャンク (構造ローカルの座標を 16 で割った物) だけを作り直す */
  | { readonly kind: 'chunks'; readonly chunks: readonly BlockPos[] }

export interface ModelUpdate extends ApplyResult {
  readonly redraw: Redraw
  /** この更新で初めて出た描く名前 (読み替えた後)。資源に無ければ作り直す (redstone-sim#234 の安全網) */
  readonly newNames: readonly string[]
}

interface Range {
  readonly min: Pos3
  readonly max: Pos3
  readonly size: BlockPos
}

function rangeOf(bounds: BoardBounds): Range {
  const axes = [bounds.x, bounds.y, bounds.z]
  for (const [lo, hi] of axes) {
    if (!Number.isInteger(lo) || !Number.isInteger(hi)) throw new RangeError(`盤面の範囲が整数でない: [${lo}, ${hi}]`)
  }
  const min = axes.map(a => a[0]) as Pos3
  const max = axes.map(a => a[1]) as Pos3
  // 最大が最小より小さい軸は大きさ 0 (何も描かない)
  const size = axes.map(a => Math.max(0, a[1] - a[0] + 1)) as BlockPos
  return { min, max, size }
}

function sameRange(a: Range, b: Range): boolean {
  return a.min.every((v, i) => v === b.min[i]) && a.max.every((v, i) => v === b.max[i])
}

export class BoardModel {
  #range: Range
  /** 世界の鍵 → 正規化した文字列。何も無いマスは持たない */
  #states = new Map<string, string>()
  #structure: CellStructure
  /** 正規化した文字列 → 描く deepslate の状態。ChunkBuilder が中のプロパティに既定の値を書き足すので、equals では引かない */
  readonly #palette = new Map<string, BlockState>()
  /** 盤面と変化に出た描く名前 (増えるだけ) */
  readonly #names = new Set<string>()

  /** 作った時の盤面を当てた結果 (IsometricView が `onBoardApplied` で 1 回知らせる) */
  readonly initial: ApplyResult

  constructor(board: Board) {
    this.#range = rangeOf(board.bounds)
    this.#structure = new CellStructure(this.#range.size)
    const u = this.setBoard(board)
    this.initial = { changed: u.changed, outside: u.outside }
  }

  /** 範囲の最小 (構造ローカルの原点の世界の座標) */
  get min(): Pos3 {
    return [...this.#range.min]
  }

  /** deepslate に渡す構造。範囲が変わると別の物になる */
  get structure(): StructureProvider {
    return this.#structure
  }

  /** 盤面と変化に出た描く名前 */
  get names(): ReadonlySet<string> {
    return this.#names
  }

  /** 世界の座標のマスの状態 (正規化した文字列)。空か範囲の外なら null */
  stateAt(x: number, y: number, z: number): string | null {
    return this.#states.get(cellKey(x, y, z)) ?? null
  }

  /** 構造ローカルの座標のマスの状態 (ピックに使う) */
  stateAtLocal(x: number, y: number, z: number): string | null {
    const m = this.#range.min
    return this.stateAt(x + m[0], y + m[1], z + m[2])
  }

  /**
   * 先に資源を作る名前 (名前順)。`preload` (アプリが渡す状態表のブロックの名前) と、盤面と変化に出た名前を合わせる。
   */
  resourceNames(preload: Iterable<string> = []): string[] {
    const out = new Set(this.#names)
    for (const p of preload) {
      const n = resourceNameOf(p)
      if (n !== null) out.add(n)
    }
    return [...out].sort()
  }

  /**
   * 盤面を差し替える。今の盤面と文字列で比べ、範囲が同じなら違うマスだけを直す。
   * 範囲が違えば丸ごと作り直す。`applyChanges` で当てた分も、この盤面に合わせて比べ直す。
   * 盤面を全部読んで確かめてから書き換えるので、途中で投げても (範囲の誤り・文字列でない値) 描く盤面は変わらない。
   */
  setBoard(board: Board): ModelUpdate {
    // 1. 読み切る。投げるならここで投げる
    const range = rangeOf(board.bounds)
    const next = new Map<string, string>()
    let outside = 0
    for (const [key, raw] of board.blocks) {
      const state = normalizeState(raw)
      const p = parseKey(key)
      if (p === null || !this.#insideOf(range, p)) {
        // 数えるのは、崩れた鍵と、範囲の外の何かあるマス (範囲の外の air は描く物が無いので数えない)
        if (state !== null || p === null) outside++
        continue
      }
      if (state !== null) next.set(cellKey(p[0], p[1], p[2]), state)
    }

    // 2. 書き換える (ここからは投げない)
    const newNames: string[] = []
    const resized = !sameRange(range, this.#range)
    if (resized) {
      let changed = 0
      for (const [k, v] of next) if (this.#states.get(k) !== v) changed++
      for (const k of this.#states.keys()) if (!next.has(k)) changed++
      this.#range = range
      this.#states = new Map()
      this.#structure = new CellStructure(range.size)
      for (const [k, v] of next) this.#put(k, parseKey(k) as Pos3, v, newNames)
      return { changed, outside, newNames, redraw: { kind: 'full', resized: true } }
    }

    const dirty = new ChunkSet(range.size)
    let changed = 0
    for (const [k, v] of next) {
      if (this.#states.get(k) === v) continue
      const p = parseKey(k) as Pos3
      this.#put(k, p, v, newNames)
      dirty.touch(this.#local(p))
      changed++
    }
    for (const k of [...this.#states.keys()]) {
      if (next.has(k)) continue
      const p = parseKey(k) as Pos3
      this.#put(k, p, null, newNames)
      dirty.touch(this.#local(p))
      changed++
    }
    return { changed, outside, newNames, redraw: dirty.redraw() }
  }

  /**
   * 変化の列を当てる。列は呼んだその場で読み切り、持ち続けない。
   *
   * - 先に列を全部読み、座標の確かめと正規化を済ませてから書き換える。列が途中で投げても
   *   (生成器の例外・文字列でない状態)、描く盤面は変わらない (作り直すチャンクと新しい名前を捨てて、絵が古いまま残らない)
   * - 同じマスが何度も出たら、最後の状態だけを当てる (A → B → A なら何も変えない。一時の名前で資源を作り直さない)
   * - 範囲の外と、座標が整数でない変化は描かずに数える (範囲の外の air は数えない。`ApplyResult.outside`)
   */
  applyChanges(changes: Iterable<CellChange>): ModelUpdate {
    // 1. 読み切る。投げるならここで投げる
    const last = new Map<string, { readonly p: Pos3; readonly state: string | null }>()
    let outside = 0
    for (const c of changes) {
      const p: Pos3 = [c.x, c.y, c.z]
      const state = normalizeState(c.state)
      const malformed = !p.every(Number.isSafeInteger)
      if (malformed || !this.#insideOf(this.#range, p)) {
        if (malformed || state !== null) outside++
        continue
      }
      last.set(cellKey(p[0], p[1], p[2]), { p, state })
    }

    // 2. 書き換える (ここからは投げない)
    const dirty = new ChunkSet(this.#range.size)
    const newNames: string[] = []
    let changed = 0
    for (const [k, { p, state }] of last) {
      if ((this.#states.get(k) ?? null) === state) continue
      this.#put(k, p, state, newNames)
      dirty.touch(this.#local(p))
      changed++
    }
    return { changed, outside, newNames, redraw: dirty.redraw() }
  }

  #insideOf(range: Range, p: Pos3): boolean {
    return p.every((v, i) => v >= range.min[i] && v <= range.max[i])
  }

  #local(p: Pos3): BlockPos {
    const m = this.#range.min
    return [p[0] - m[0], p[1] - m[1], p[2] - m[2]]
  }

  /** 1 マスを書く (null で消す)。初めて出た描く名前を `newNames` に足す */
  #put(key: string, world: Pos3, state: string | null, newNames: string[]): void {
    const local = this.#local(world)
    if (state === null) {
      this.#states.delete(key)
      this.#structure.set(local, null)
      return
    }
    this.#states.set(key, state)
    let draw = this.#palette.get(state)
    if (draw === undefined) {
      draw = drawStateOf(state)
      this.#palette.set(state, draw)
    }
    const name = drawNameOf(blockNameOf(state))
    if (!this.#names.has(name)) {
      this.#names.add(name)
      newNames.push(name)
    }
    this.#structure.set(local, draw)
  }
}

/**
 * 作り直すチャンクの集まり。変わったマスのチャンクと、マスがチャンクの面に接していれば、その向こうのチャンクも入れる
 * (隣のマスの面の隠し (ChunkBuilder.needsCull) が、このマスで決まるため。見るのは 6 方向の面の隣だけ)。
 */
class ChunkSet {
  readonly #size: BlockPos
  readonly #counts: BlockPos
  readonly #set = new Map<number, BlockPos>()
  #any = false

  constructor(size: BlockPos) {
    this.#size = size
    this.#counts = size.map(s => Math.ceil(s / CHUNK_SIZE)) as BlockPos
  }

  #add(c: BlockPos): void {
    const n = this.#counts
    this.#set.set((c[0] * n[1] + c[1]) * n[2] + c[2], c)
  }

  /** 構造ローカルのマスが変わった */
  touch(local: BlockPos): void {
    this.#any = true
    const c = local.map(v => Math.floor(v / CHUNK_SIZE)) as BlockPos
    this.#add(c)
    for (let axis = 0; axis < 3; axis++) {
      const r = local[axis] % CHUNK_SIZE
      if (r === 0 && local[axis] > 0) this.#add(c.map((v, i) => (i === axis ? v - 1 : v)) as BlockPos)
      if (r === CHUNK_SIZE - 1 && local[axis] + 1 < this.#size[axis]) this.#add(c.map((v, i) => (i === axis ? v + 1 : v)) as BlockPos)
    }
  }

  redraw(): Redraw {
    if (!this.#any) return { kind: 'none' }
    const total = this.#counts[0] * this.#counts[1] * this.#counts[2]
    if (this.#set.size > Math.min(MAX_CHUNK_LIST, total / 2)) return { kind: 'full', resized: false }
    return { kind: 'chunks', chunks: [...this.#set.values()] }
  }
}

// ── 描く盤面と絵のつなぎ ──────────────────────────────────────────────────

/** 絵 (deepslate の StructureRenderer) のうち、BoardView が呼ぶ口。試験では呼ばれた口を記録する偽物を渡す */
export interface RedrawTarget {
  setStructure(structure: StructureProvider): void
  updateStructureBuffers(chunkPositions?: BlockPos[]): void
}

/** BoardView が IsometricView に頼む事 */
export interface ViewHooks {
  /** 資源を作る名前が増えた。資源を作り直し、できたら `attach` する (それまで絵は外してある) */
  names(names: readonly string[]): void
  /** 範囲が変わり、絵の構造を差し替えた。寄りを合わせ直す */
  resized(size: BlockPos): void
  /** 次のフレームで描く */
  draw(): void
}

/** 盤面の変化を絵へ当てた道 */
export type RedrawStep =
  /** 何も変わらない */
  | 'none'
  /** 絵がまだ無い (読み込み中)。`attach` の時に、その時の構造から作る */
  | 'loading'
  /** 知らない名前が出た。絵を外して資源を作り直す */
  | 'names'
  /** 変わったチャンクだけを作り直した */
  | 'chunks'
  /** 全部を作り直した (範囲は同じ) */
  | 'all'
  /** 範囲が変わった。構造を差し替え、寄りを合わせ直した */
  | 'resized'

export interface ViewUpdate {
  readonly result: ApplyResult
  readonly step: RedrawStep
}

/**
 * 描く盤面 (BoardModel) と絵の間の振り分け。IsometricView はこれを持ち、絵 (StructureRenderer) を
 * 作った時に `attach`、作り直す時と外す時に `detach` するだけにする。React と WebGL に依らないので、
 * 偽の絵で試せる (読み込み中の変化・チャンクだけ・全部・範囲の変更と寄り直し・新しい名前での作り直し)。
 */
export class BoardView<R extends RedrawTarget = RedrawTarget> {
  readonly model: BoardModel
  readonly #hooks: ViewHooks
  #renderer: R | null = null
  #names: readonly string[]
  #preload: readonly string[]

  constructor(board: Board, preload: readonly string[], hooks: ViewHooks) {
    this.model = new BoardModel(board)
    this.#hooks = hooks
    this.#preload = preload
    this.#names = this.model.resourceNames(preload)
  }

  /** 今の絵。読み込み中と、資源を作り直している間は null */
  get renderer(): R | null {
    return this.#renderer
  }

  /** 先に資源を作る名前 (名前順・増えるだけ) */
  get resourceNames(): readonly string[] {
    return this.#names
  }

  /** `preloadNames` が変わった。名前が増えたら資源を作り直す (true) */
  setPreload(preload: readonly string[]): boolean {
    this.#preload = preload
    return this.#syncNames()
  }

  setBoard(board: Board): ViewUpdate {
    return this.#apply(this.model.setBoard(board))
  }

  applyChanges(changes: Iterable<CellChange>): ViewUpdate {
    return this.#apply(this.model.applyChanges(changes))
  }

  /**
   * 資源ができた時に呼ぶ。**その時の構造から**絵を作る (読み込み中に当てた盤面と変化も入る。redstone-sim#109)。
   */
  attach(make: (structure: StructureProvider) => R): R {
    const r = make(this.model.structure)
    this.#renderer = r
    return r
  }

  /** 絵を外す (資源の作り直し・unmount)。`renderer` を渡すと、それが今の絵の時だけ外す */
  detach(renderer?: R): void {
    if (renderer === undefined || this.#renderer === renderer) this.#renderer = null
  }

  /** 名前が増えたら絵を外し、資源の作り直しを頼む */
  #syncNames(): boolean {
    const merged = this.model.resourceNames([...this.#names, ...this.#preload])
    // 今の名前を全部含めて合わせたので、増えたかは数で分かる
    if (merged.length === this.#names.length) return false
    this.#names = merged
    this.#renderer = null
    this.#hooks.names(merged)
    return true
  }

  #apply(u: ModelUpdate): ViewUpdate {
    const result: ApplyResult = { changed: u.changed, outside: u.outside }
    if (u.newNames.length > 0 && this.#syncNames()) return { result, step: 'names' }
    const r = this.#renderer
    if (u.redraw.kind === 'none') return { result, step: 'none' }
    if (r === null) return { result, step: 'loading' }
    let step: RedrawStep
    if (u.redraw.kind === 'chunks') {
      r.updateStructureBuffers(u.redraw.chunks.map(c => [c[0], c[1], c[2]]))
      step = 'chunks'
    } else if (u.redraw.resized) {
      r.setStructure(this.model.structure)
      this.#hooks.resized(this.model.structure.getSize())
      step = 'resized'
    } else {
      r.updateStructureBuffers()
      step = 'all'
    }
    this.#hooks.draw()
    return { result, step }
  }
}
