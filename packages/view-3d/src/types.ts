/**
 * view-3d の窓口の型。
 *
 * 盤面・変化・押した時の扱いの型は board にある (#14。docs/architecture.md §1。view-3d は board の型だけに依る)。
 * ここでは view-3d の中の `./types` の import のために出し直し、自分では描く側の結果 (`ApplyResult`) だけを持つ。
 * アプリは盤面の型を board から読む (view-3d の index.ts は board の型を書き出さない)。
 *
 * 盤面を描く時の決まり (board の `Board` の注の「描く側」は view-3d のこと):
 * - 先頭の `minecraft:` の有無とプロパティの並びは `normalizeState` でそろえる。`PickRoleFn` にも、その形で渡す。
 *   押せるかを素通りの一覧で済ませるなら、`PickRoleFn` は `pickRoleFrom(isTarget)` で作る
 * - 鍵は、3 つの部分がどれも `/^-?\d+$/` の字面なら整数として読んで描く (board の `cellKey` の字面より緩い。
 *   `"01,0,0"` も 1,0,0 として描く)。範囲の外のマスと、この形でない鍵のマスは描かず、
 *   その数を `IsometricView` の `onBoardApplied` で知らせる (`ApplyResult.outside`)
 */

export type { Board, BoardBounds, CellChange, PickRole, PickRoleFn, Pos3 } from '@redstone-engine/board'

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
