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

/** `"x,y,z"` を座標にする。3 つの整数でなければ null */
function parseKey(key: string): Pos3 | null {
  const parts = key.split(',')
  if (parts.length !== 3) return null
  const p = parts.map(v => (v.trim() === '' ? NaN : Number(v))) as Pos3
  return p.every(Number.isInteger) ? p : null
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
 * どちらの値も、手元の計測ではなく見積もり。
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

  constructor(board: Board) {
    this.#range = rangeOf(board.bounds)
    this.#structure = new CellStructure(this.#range.size)
    this.setBoard(board)
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
   */
  setBoard(board: Board): ModelUpdate {
    const range = rangeOf(board.bounds)
    const next = new Map<string, string>()
    let outside = 0
    for (const [key, raw] of board.blocks) {
      const state = normalizeState(raw)
      const p = parseKey(key)
      if (p === null || !this.#insideOf(range, p)) {
        if (state !== null || p === null) outside++
        continue
      }
      if (state !== null) next.set(cellKey(p[0], p[1], p[2]), state)
    }

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
   * 範囲の外と、座標が整数でない変化は描かずに数える。同じマスが何度も出たら、後の物が残る。
   */
  applyChanges(changes: Iterable<CellChange>): ModelUpdate {
    const dirty = new ChunkSet(this.#range.size)
    const newNames: string[] = []
    let changed = 0
    let outside = 0
    for (const c of changes) {
      const p: Pos3 = [c.x, c.y, c.z]
      if (!p.every(Number.isInteger) || !this.#insideOf(this.#range, p)) {
        outside++
        continue
      }
      const state = normalizeState(c.state)
      const k = cellKey(p[0], p[1], p[2])
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
