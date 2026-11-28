/**
 * 盤面の値の操作: 範囲・床・鍵・エンジンへ読み込む列・変化の当て方・名前の一覧・全部切れか。
 *
 * どれも純粋な関数で、渡された盤面を書き換えない。ダストの形・点灯・電力は計算しない
 * (置いた時の状態はエンジンの置く入力が決め、盤面は返った変化の列で直すだけ。#13)。
 */

import type { Board, BoardBounds, CellChange, Pos3 } from './types'

// ── 範囲と床 ──────────────────────────────────────────────────────────────

/** 床の段 (y)。石を敷き詰め、編集しない */
export const FLOOR_Y = 0

/** 編集する段 (y)。置く・消すのはこの段だけ */
export const EDIT_Y = 1

/**
 * 盤面の範囲: x・z は 0〜15、y は床 (0) と編集する段 (1)。
 * 原点は conformance/fixtures の回路と同じ (床の段が y=0、素子の段が y=1)。
 */
export const BOARD_BOUNDS: BoardBounds = Object.freeze({
  x: Object.freeze([0, 15] as const),
  y: Object.freeze([FLOOR_Y, EDIT_Y] as const),
  z: Object.freeze([0, 15] as const),
})

/**
 * 床のブロック。満杯の立方体なので上面が頑丈で、ダストの支えになる (V06 §4.5)。
 * 床置きのレバーも、支え (真下) の面が sturdy でないと残らない (V09 §4.1)。
 * 床はエンジンへも読み込む (`cellsToLoad`)。読み込まないと、エンジンの世界では支えが無い
 */
export const FLOOR_STATE = 'stone'

/** 床だけの盤面 (y=0 に石を 256 マス)。返すたびに新しい表を作る */
export function createBoard(): Board {
  const blocks = new Map<string, string>()
  const [x0, x1] = BOARD_BOUNDS.x
  const [z0, z1] = BOARD_BOUNDS.z
  for (let z = z0; z <= z1; z++) {
    for (let x = x0; x <= x1; x++) blocks.set(cellKey(x, FLOOR_Y, z), FLOOR_STATE)
  }
  return { bounds: BOARD_BOUNDS, blocks }
}

/** 範囲の中か。どの座標も安全な整数でなければ外 */
function inside(b: BoardBounds, x: number, y: number, z: number): boolean {
  const within = (v: number, [lo, hi]: readonly [number, number]) => Number.isSafeInteger(v) && v >= lo && v <= hi
  return within(x, b.x) && within(y, b.y) && within(z, b.z)
}

/** 編集できるマスか: 盤面の範囲の中で、y が編集する段 (1) の時だけ。範囲の外と床 (y=0) は断る */
export function isEditable(x: number, y: number, z: number): boolean {
  return y === EDIT_Y && inside(BOARD_BOUNDS, x, y, z)
}

// ── 鍵 ────────────────────────────────────────────────────────────────────

/** 盤面の鍵 `"x,y,z"` (描く側の鍵と同じ形) */
export function cellKey(x: number, y: number, z: number): string {
  return `${x},${y},${z}`
}

/** 鍵を座標にする。`cellKey` が作る字面 (10 進の整数で、前の 0・`-0`・`+`・空白なし) でなければ null */
function cellOf(key: string): Pos3 | null {
  const parts = key.split(',')
  if (parts.length !== 3) return null
  const p = parts.map(Number) as Pos3
  return p.every(Number.isSafeInteger) && cellKey(p[0], p[1], p[2]) === key ? p : null
}

// ── 状態の文字列 ──────────────────────────────────────────────────────────

/** 何も無いマスとして扱う名前 */
const EMPTY_NAMES: ReadonlySet<string> = new Set(['air', 'cave_air', 'void_air'])

const NAMESPACE = 'minecraft:'

/** ブロックの名前 (先頭の `minecraft:` と `[…]` を外した物) */
export function blockNameOf(state: string): string {
  const s = state.startsWith(NAMESPACE) ? state.slice(NAMESPACE.length) : state
  const open = s.indexOf('[')
  return open < 0 ? s : s.slice(0, open)
}

/** 何も無いマスか (air・cave_air・void_air) */
export function isEmptyState(state: string): boolean {
  return EMPTY_NAMES.has(blockNameOf(state))
}

/** プロパティの値 (`name[k=v,…]` の k)。無ければ undefined */
function propOf(state: string, key: string): string | undefined {
  const open = state.indexOf('[')
  if (open < 0 || !state.endsWith(']')) return undefined
  for (const kv of state.slice(open + 1, -1).split(',')) {
    const eq = kv.indexOf('=')
    if (eq > 0 && kv.slice(0, eq) === key) return kv.slice(eq + 1)
  }
  return undefined
}

// ── エンジンとの受け渡し ──────────────────────────────────────────────────

/**
 * エンジンへ読み込む列。何も無いマスは入れない。
 *
 * 並びは決まった順 (y・z・x の小さい順)。床 (y=0) が先に入り、同じ段は fixture の `blocks` と同じ並びになる。
 * 鍵が `cellKey` の形でないマスと、範囲の外のマスがあれば RangeError (盤面とエンジンの世界がずれるので、黙って落とさない)
 */
export function cellsToLoad(board: Board): CellChange[] {
  const out: CellChange[] = []
  for (const [key, state] of board.blocks) {
    if (isEmptyState(state)) continue
    const p = cellOf(key)
    if (p === null) throw new RangeError(`盤面の鍵が "x,y,z" の形でない: ${key}`)
    if (!inside(board.bounds, p[0], p[1], p[2])) throw new RangeError(`盤面の範囲の外のマス: ${key}`)
    out.push({ x: p[0], y: p[1], z: p[2], state })
  }
  return out.sort((a, b) => a.y - b.y || a.z - b.z || a.x - b.x)
}

/**
 * 変化の列を盤面の写しへ当てた、新しい盤面。渡した盤面は書き換えない。
 *
 * - 何も無いマス (air・cave_air・void_air) への変化は、表から消す
 * - 範囲の外のマスと、座標が整数でない変化は当てない (盤面はエンジンの世界のうち範囲の中の写し)
 * - 同じマスが列に何度出ても、最後の状態だけを当てる (A → B → A なら変わらない)
 * - 何も変わらなければ、渡した盤面そのもの (同じ参照) を返す。描く側は同じ参照なら比べ直さない
 */
export function applyChanges(board: Board, changes: Iterable<CellChange>): Board {
  const last = new Map<string, string | undefined>()
  for (const c of changes) {
    if (!inside(board.bounds, c.x, c.y, c.z)) continue
    last.set(cellKey(c.x, c.y, c.z), isEmptyState(c.state) ? undefined : c.state)
  }
  let next: Map<string, string> | null = null
  for (const [key, state] of last) {
    if (board.blocks.get(key) === state) continue
    next ??= new Map(board.blocks)
    if (state === undefined) next.delete(key)
    else next.set(key, state)
  }
  return next === null ? board : { bounds: board.bounds, blocks: next }
}

// ── 見る ──────────────────────────────────────────────────────────────────

/** 盤面に出るブロックの名前 (プロパティを外した物) の一覧。重ならず、名前順。何も無いマスは入れない */
export function blockNamesOf(board: Board): string[] {
  const names = new Set<string>()
  for (const state of board.blocks.values()) {
    if (!isEmptyState(state)) names.add(blockNameOf(state))
  }
  return [...names].sort()
}

/**
 * 全部切れか (試験用)。点いている物が 1 つも無ければ true:
 * レバーは `powered=true` でない・ダストは `power` が 0 か無い・ランプは `lit=true` でない。ほかのブロックは見ない
 */
export function isAllOff(board: Board): boolean {
  for (const state of board.blocks.values()) {
    switch (blockNameOf(state)) {
      case 'lever':
        if (propOf(state, 'powered') === 'true') return false
        break
      case 'redstone_wire': {
        const power = propOf(state, 'power')
        if (power !== undefined && power !== '0') return false
        break
      }
      case 'redstone_lamp':
        if (propOf(state, 'lit') === 'true') return false
        break
    }
  }
  return true
}
