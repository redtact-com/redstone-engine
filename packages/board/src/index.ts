/**
 * board: 盤面の型と、盤面の値の操作。エンジンにも描く側にも依らない (依存は無し。docs/architecture.md §1)。
 *
 * ```ts
 * let board = createBoard()                    // y=0 に石の床 256 マス (エンジンへも読み込む)
 * const cells = cellsToLoad(board)             // 決まった順。state は StateTable.name の形の文字列
 *
 * // 置く: 編集できるマスで、同じブロックが無ければ、置きたい状態をエンジンの置く入力へ渡す
 * const wanted = PLACE_STATES.dust
 * if (isEditable(x, y, z) && !isSameBlock(board.blocks.get(cellKey(x, y, z)), wanted)) {
 *   // エンジンを呼び、変化の窓を読み切って list にする (アプリの編集の流れ。#15)
 *   board = applyChanges(board, list)          // 変わらなければ同じ参照が返る
 * }
 * ```
 *
 * - ダストの形・点灯・電力を計算しない。置いた時の状態はエンジンの置く入力 (#13) が決める
 * - エンジンを呼ぶ編集の流れ・「選ぶ」・保存・回路ファイルの読み書きは持たない
 * - 書き出す名前は下の一覧に固定する (`export *` を使わない。test/guard.test.ts が見張る)
 */

export type { Board, BoardBounds, CellChange, PickRole, PickRoleFn, Pos3 } from './types'

export {
  BOARD_BOUNDS, EDIT_Y, FLOOR_STATE, FLOOR_Y,
  applyChanges, blockNamesOf, cellKey, cellsToLoad, createBoard, isAllOff, isEditable,
} from './board'

export { PLACE_STATES, TOOLS, isSameBlock } from './tools'
export type { PlaceTool, Tool } from './tools'
