/**
 * 道具: レバー・ダスト・ランプ・消しゴム。置く道具は、エンジンの置く入力 (#13) へ渡す「置きたい状態」を持つ。
 *
 * 置きたい状態は、プロパティを全部書いた `StateTable.name` の形 (名前順・`minecraft:` なし) の文字列にする。
 * 置いた後の本当の状態 (ダストの形と power・ランプの lit) はエンジンが置く時に決め、盤面は返った変化で直す。
 * 置き換えの手順 (壊す → 置く) とエンジンの呼び出しは、アプリの編集の流れ (#15) が持つ。
 */

import { blockNameOf, isEmptyState } from './board'

/** 道具の種類 */
export type Tool = 'lever' | 'dust' | 'lamp' | 'eraser'

/** 置く道具 (消しゴムのほか) */
export type PlaceTool = Exclude<Tool, 'eraser'>

/** 道具の並び (道具の棚に出す順) */
export const TOOLS: readonly Tool[] = Object.freeze(['lever', 'dust', 'lamp', 'eraser'] as const)

/**
 * 置く道具ごとの、置きたい状態。
 *
 * - レバー: 床置き・north・切。プロパティは face・facing・powered (V09 §3.1。既定は壁置きなので、床置きを書く)。
 *   床置きの取り付け方向は facing によらず上で、支え (真下) を強充電する向きも facing によらない (V09 §3.1・§4.1)。
 *   ダストの接続も facing によらない (レバーは信号源なので、4 方向とも接続先になる。V06 §4.1)。
 *   信号と更新のうち facing を使うのは、実験的レッドストーンが有効な時の更新の向き (Orientation) だけ (V09 §4.1) なので、
 *   実験的レッドストーンを使わない世界では north に決めてよい。fixture の lever-wire-lamp と同じ文字列
 * - ダスト: 既定の状態 (4 方向とも none・power 0。V06 §3.1)。置いた時の形は周りから決まり、power は 0 (V06 §4.4)
 * - ランプ: 既定の状態 (lit=false)。置いた時の lit は、受電しているかで決まる (V11 §4.1)
 */
export const PLACE_STATES: Readonly<Record<PlaceTool, string>> = Object.freeze({
  lever: 'lever[face=floor,facing=north,powered=false]',
  dust: 'redstone_wire[east=none,north=none,power=0,south=none,west=none]',
  lamp: 'redstone_lamp[lit=false]',
})

/**
 * 同じブロックか。ブロックの名前 (`minecraft:` とプロパティを外した物) で比べる。
 * 何も無いマス (undefined・air・cave_air・void_air) どうしも同じとする。
 *
 * ダストは周りとの形で文字列が変わるので、文字列で比べると、ダストの道具でなぞるたびに置き直してしまう。
 * 名前で比べれば、形の違うダストどうしは同じになる。
 */
export function isSameBlock(a: string | undefined, b: string | undefined): boolean {
  const nameOf = (s: string | undefined) => (s === undefined || isEmptyState(s) ? null : blockNameOf(s))
  return nameOf(a) === nameOf(b)
}
