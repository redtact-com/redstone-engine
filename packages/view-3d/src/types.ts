/**
 * view-3d の窓口の型。
 *
 * 盤面は **blockstate の文字列** で受ける (engine-js の `StateTable.name` の形)。エンジンの型にも
 * deepslate の型にも依らないので、描画の中身を替えても窓口は変わらない。
 * board のパッケージができるまではここに置き、6 本目で board へ移す (docs/architecture.md §1。view-3d は board の型には依ってよい)。
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
 * 描く盤面。
 *
 * - `blocks` の鍵は `"x,y,z"` (世界の座標。`cellKey` で作る)、値は blockstate の文字列。
 *   先頭の `minecraft:` は有っても無くてもよく、プロパティの並びも問わない (`normalizeState` でそろえる)。
 * - air・cave_air・void_air は何も無いマスとして扱う (書かなくてもよい)。
 * - 範囲の外のマスと、鍵が `"x,y,z"` の形 (どの部分も `/^-?\d+$/`) でないマスは描かない。
 *   その数は `IsometricView` の `onBoardApplied` で知らせる (`ApplyResult.outside`)。
 */
export interface Board {
  readonly bounds: BoardBounds
  readonly blocks: ReadonlyMap<string, string>
}

/**
 * 1 マスの変化。`state` は blockstate の文字列 (`Board.blocks` の値と同じ決まり)。
 * engine-js の変化の窓から作る時は、窓を読み切ってから渡す (窓は次に窓口を呼ぶと使えない。docs/wasm-abi.md §4)。
 */
export interface CellChange {
  readonly x: number
  readonly y: number
  readonly z: number
  readonly state: string
}

/** 盤面の差し替え (`onBoardApplied`) と `applyChanges` の結果 */
export interface ApplyResult {
  /**
   * 描く盤面の中で、状態が変わったマスの数。同じマスが列に何度出ても 1 つと数え、最後の状態だけを当てる
   * (A → B → A なら 0)
   */
  readonly changed: number
  /**
   * 描かなかった件数。範囲の外の何かあるマス (範囲の外の air は描く物が無いので数えない) と、
   * 座標が崩れた物 (鍵が `"x,y,z"` の形でない・座標が整数でない。air でも数える)。件数なので、同じマスが 2 回出れば 2
   */
  readonly outside: number
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
 * マスの状態 (正規化した blockstate の文字列。`normalizeState` の形) から扱いを決める関数。
 * 押せるかどうかはアプリが決める。素通りの一覧で済ませるなら `pickRoleFrom(isTarget)` で作る。
 */
export type PickRoleFn = (state: string) => PickRole
