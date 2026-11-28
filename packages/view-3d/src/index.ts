/**
 * view-3d: 盤面を 3D (と真上からの 2D) で描き、押した座標を返す。
 *
 * 出どころ: redtact-com/redstone-sim の packages/viewer (develop a40ae23。viewer を最後に変えたのは
 * redstone-sim#436 の b9e48ee)。描画は deepslate 0.25.1 のまま、入力を TS のエンジンの型 (WorldSnapshot・BlockState)
 * から **blockstate の文字列と変化の列** に変えた (#11)。
 *
 * ```tsx
 * const viewRef = useRef<IsometricViewHandle | null>(null)
 * <IsometricView board={board} preloadNames={names} pickRole={pickRoleFrom(isTarget)} viewRef={viewRef}
 *   onBlockClick={([x, y, z]) => onUse(x, y, z)} />
 *
 * // 押されたら、エンジンへ渡して変化を描く (変化の窓は次に窓口を呼ぶ前に読み切る)
 * world.use(x, y, z)
 * const w = world.changes()
 * const list = []
 * for (let i = 0; i < w.count; i++) list.push({ x: w.x(i), y: w.y(i), z: w.z(i), state: states.name(w.state(i)) })
 * viewRef.current?.applyChanges(list)
 * ```
 *
 * - engine-js と @redstone/sim に依らない。board には型だけ依る。盤面・変化・押した時の扱いの型 (`Board`・`CellChange`・
 *   `PickRoleFn` ほか) は board から読み、ここでは描く側の物 (`ApplyResult` ほか) だけを書き出す (#14)。
 *   窓口 (props・命令の口・書き出す型) に deepslate の型を出さない
 * - 押せるかの意味は持たない。アプリが `pickRole` で渡す (素通りの一覧と「一覧に無い物は遮る」だけを持つ)
 * - テクスチャとモデルは同梱しない。実行時に misode/mcmeta と PrismarineJS/minecraft-assets から取る
 *   (raw.githubusercontent.com → cdn.jsdelivr.net。docs/ip-policy.md §2)。取れなければ単色で描く
 */

export type { ApplyResult } from './types'

export { IsometricView } from './IsometricView'
export type { IsometricViewProps, IsometricViewHandle, CameraState, CameraInput } from './IsometricView'

export { cellKey, normalizeState } from './board-to-structure'
export { pickRoleFrom, PICK_PASS_THROUGH } from './renderer/picking'
export { fitDistance, maxZoomOut, VIEWER_FOV_DEG, VIEWER_FAR, VIEWER_NEAR } from './camera'

// パレットのアイコンなどアプリ側からも参照する (redstone-sim#276)。**定数を 2 か所に置かない**
export { MCMETA_BASE } from './renderer/mcAssets'
export type { AssetState } from './renderer/buildResources'
