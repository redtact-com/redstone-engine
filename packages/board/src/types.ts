/**
 * 盤面の窓口の型。アプリ・描く側 (3D の描画)・エンジンへ渡す列が、同じ型で盤面を受け渡す。
 *
 * 盤面は **blockstate の文字列** で持つ (engine-js の `StateTable.name` の形)。エンジンの型にも描画の型にも
 * 依らないので、どちらの中身を替えても窓口は変わらない。5 本目 (#11) で描く側に仮に置いた型を移した (#14)。
 */

/** 世界の座標 [x, y, z] */
export type Pos3 = [number, number, number]

/** 範囲。どの軸も [最小, 最大] で、両端を含む */
export interface BoardBounds {
  readonly x: readonly [number, number]
  readonly y: readonly [number, number]
  readonly z: readonly [number, number]
}

/**
 * 盤面。範囲と、マスの表。
 *
 * - `blocks` の鍵は `"x,y,z"` (世界の座標。`cellKey` で作る)、値は blockstate の文字列。
 *   board が作る値 (床・道具の置きたい状態) は `StateTable.name` の形 (プロパティを名前順で全部書き、`minecraft:` を付けない)。
 *   変化を当てる時 (`applyChanges`) は、渡された文字列をそのまま入れる。
 *   描く側は、先頭の `minecraft:` の有無とプロパティの並びを問わずにそろえて読む。
 * - air・cave_air・void_air は何も無いマスとして扱う (書かなくてもよい)。
 * - 範囲の外のマスと、鍵が `"x,y,z"` の形 (どの部分も `/^-?\d+$/`) でないマスは描かない。
 *   描く側は、描かなかった数を知らせる。
 */
export interface Board {
  readonly bounds: BoardBounds
  readonly blocks: ReadonlyMap<string, string>
}

/**
 * 1 マスの変化。`state` は blockstate の文字列 (`Board.blocks` の値と同じ決まり)。
 * engine-js の変化の窓から作る時は、窓を読み切ってから渡す (窓は次に窓口を呼ぶと使えない。docs/wasm-abi.md §4)。
 * エンジンへ読み込む列 (`cellsToLoad`) も、この形で返す。
 */
export interface CellChange {
  readonly x: number
  readonly y: number
  readonly z: number
  readonly state: string
}

/**
 * 3D で押した時の、マスの扱い。
 *  - `target`: 押せる。1 マス全体を当たりにする (絵が数 px のレバーも押せるように)
 *  - `pass`:   素通り。細い・透けるので奥の物を隠さない。それ自体は押せない
 *  - `block`:  遮る。奥の物は押せない (見えない物は押せない)
 *
 * 空のマスは素通りで、`PickRoleFn` を呼ばない。
 */
export type PickRole = 'target' | 'pass' | 'block'

/**
 * マスの状態から扱いを決める関数。押せるかどうかはアプリが決める。
 * 受ける文字列は、描く側がそろえた形 (`minecraft:` を外し、プロパティを名前順に並べた blockstate の文字列)。
 */
export type PickRoleFn = (state: string) => PickRole
