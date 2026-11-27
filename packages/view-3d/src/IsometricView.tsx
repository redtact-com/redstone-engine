/**
 * IsometricView — 盤面 (blockstate の文字列) を deepslate で描く React の部品。
 *
 * - 盤面は `board` (範囲と「`"x,y,z"` → 文字列」の表) で受け、変化は `viewRef` の `applyChanges` で受ける。
 *   どちらも描く盤面 (board-to-structure.ts の BoardModel) へ当て、変わったマスのチャンクだけを作り直す。
 * - 読み込み中 (資源を作っている間) に来た盤面と変化も捨てない。準備ができた時の盤面から構造を作る。
 * - 3D で押せる物はアプリが `pickRole` で決める。座標は 3D でも真上からの 2D でも世界の座標。
 *
 * 移す元は redstone-sim の packages/viewer/src/IsometricView.tsx (develop a40ae23)。
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import type * as React from 'react'
import { StructureRenderer } from 'deepslate/render'
import { BoardModel, type ModelUpdate } from './board-to-structure'
import type { ApplyResult, Board, CellChange, PickRoleFn, Pos3 } from './types'
import { buildResources, type AssetState } from './renderer/buildResources'
import { fitDistance, maxZoomOut } from './camera'
import { useCamera } from './renderer/useCamera'
import { canvasPixelToBlock, FOV_F } from './renderer/coordUtils'
import {
  viewMatrixOf, projMatrixOf, canvasRay, pointToCanvas, pickCell, pickNearest, pickRoleFrom,
  TOUCH_PICK_RADII,
} from './renderer/picking'

// ─── 3D のクリック・タップの見分け (redstone-sim#397) ─────────────────────────

/** クリック・タップとみなす動きの上限 (CSS px)。越えたらドラッグ (カメラ回転) */
const CLICK_SLOP_MOUSE = 5
const CLICK_SLOP_TOUCH = 10
/** タッチでこれより長く押したらタップにしない (長押し)。マウスは時間を見ない */
const TAP_MAX_MS_TOUCH = 500

/** `pickRole` を渡さない時: 何も押せない (素通りの一覧のほかは遮る) */
const NO_TARGETS: PickRoleFn = pickRoleFrom(() => false)

// ─── Props ────────────────────────────────────────────────────────────────────

export interface CameraState {
  distance: number
  panX: number
  panZ: number
}

/** 外部からカメラを 1 回だけ上書きするためのパッチ (デモの fitCamera 用)。 */
export interface CameraInput {
  distance?: number
  panX?: number
  panZ?: number
  rotX?: number
  rotY?: number
}

/** IsometricView を外から命令的に使う口 (redstone-sim#397)。`viewRef` に入る */
export interface IsometricViewHandle {
  /**
   * ブロックの中心が画面のどこに描かれているか (clientX/Y)。座標は世界の座標。
   * 描画の準備ができていない・カメラの後ろなら null。
   * e2e が実際の座標をクリック・タップして確かめるために使う
   */
  blockToClient: (pos: Pos3) => { x: number; y: number } | null
  /**
   * 次のフレームで 1 回描く (redstone-sim#434)。`frameLoop="demand"` の間に、ビューアが自分では気づけない
   * 変更 (`cameraInputRef` に値を入れた、など) を伝えるために呼ぶ。`"always"` では何もしなくてよい
   */
  invalidate: () => void
  /**
   * 変化の列 (1 件は `{ x, y, z, state }`。座標は世界の座標) を描く盤面へ当てる。
   * 列は呼んだその場で読み切る。engine-js の変化の窓は次に窓口を呼ぶと使えないので、窓そのものは渡さない
   * (`StateTable.name` で文字列にした列を渡す。docs/wasm-abi.md §4)。
   * 範囲の外の変化は描かずに数を返す。読み込み中に呼んでも捨てず、準備ができた時に描く
   */
  applyChanges: (changes: Iterable<CellChange>) => ApplyResult
  /** いま描いているマスの状態 (正規化した文字列。`normalizeState` の形)。空か範囲の外なら null。テスト用 */
  stateAt: (pos: Pos3) => string | null
}

export interface IsometricViewProps {
  /**
   * 描く盤面。変わったら、いま描いている盤面と文字列で比べ、範囲が同じなら違うマスだけを直す
   * (範囲が違えば丸ごと作り直し、寄りも合わせ直す)。`applyChanges` で当てた分も、この盤面に合わせて比べ直す。
   * 同じ物 (参照) を渡し直しても何もしない
   */
  board: Board
  /**
   * 先に資源を作るブロックの名前 (アプリが状態表のブロックの名前を渡す)。盤面と変化に出た名前は自分で集め、
   * 知らない名前が出たら資源を作り直す (redstone-sim#234 の安全網)。`minecraft:` とプロパティは有ってもよい
   */
  preloadNames?: readonly string[]
  /**
   * 3D で押した時のマスの扱い。受ける文字列は正規化した blockstate の文字列で、空のマスでは呼ばない。
   * 押せる物だけを決めるなら `pickRoleFrom(isTarget)` で作る。省くと何も押せない
   */
  pickRole?: PickRoleFn
  /** true にすると真上からの 2D ビュー */
  topDown?: boolean
  /**
   * クリック・タップされたブロックの座標を知らせる。**座標は世界の座標** (盤面の鍵と同じ)。
   *
   * - topDown=true: 左右クリックを区別して返す。y は placementY
   * - topDown=false (3D, redstone-sim#397): `pickRole` が target と言ったマスに当たった時だけ 'left' で返す。
   *   ドラッグ (カメラ回転)・ピンチ・タッチの長押しでは呼ばない。
   *   渡さなければ、3D ではピックもカーソルの変更もしない
   */
  onBlockClick?: (pos: Pos3, button: 'left' | 'right') => void
  /** 2D の時にクリックの判定に使う層の y (世界の座標)。省くと範囲の下から 2 段目 */
  placementY?: number
  /**
   * topDown グリッドオーバーレイ用。
   * 毎フレームカメラ状態を書き込む ref を渡すと、外部からカメラ状態を参照できる。
   */
  cameraStateRef?: React.MutableRefObject<CameraState | null>
  /** true のとき左クリック/タッチドラッグで平面パン（移動ツール選択時） */
  panMode?: boolean
  /**
   * 外部からカメラを命令的に上書きするための ref。値をセットすると次フレームで
   * 適用され null に消費される (デモの fitCamera など)。
   */
  cameraInputRef?: React.MutableRefObject<CameraInput | null>
  /**
   * テクスチャ読込 + 構造の構築が済み、最初の絵を描いた時に呼ぶ (資源を作り直すたびに 1 回)。
   * 読み込み中の変化は捨てないので、ここで描き直しを頼まなくてよい (redstone-sim#109 の描き直しは要らない)
   */
  onReady?: () => void
  /** 命令的な口 (redstone-sim#397)。ref を渡すと、マウント中は中身が入る (cameraInputRef と同じ形) */
  viewRef?: React.MutableRefObject<IsometricViewHandle | null>
  /**
   * 操作を受けるか (redstone-sim#434)。既定は true (今までどおり)。
   *
   * false の間は wheel と touch を奪わない: wheel を止めず (ビューアの上でもページが流れる)、
   * `touch-action` を外し (指でなぞるとページが流れる)、ドラッグ・ピンチ・クリックにも反応しない。
   * 記事の途中に置くとき、押して有効にするまでページのスクロールを邪魔しないために使う。
   * click などの DOM イベントは今までどおり親へ伝わるので、有効にする操作は親で受けられる
   */
  interactive?: boolean
  /**
   * リソースパックの基点 URL (redstone-sim#434)。**既定は無し** (#11): パックへは取りに行かず、
   * mcmeta と PrismarineJS だけから描く。渡すと、基点の下の `bundle.json` と `assets/minecraft/...` を先に試す。
   * 変えると資源を取り直して作り直す
   */
  resourcePackUrl?: string
  /**
   * いつ描くか (redstone-sim#434)。
   * - `"always"` (既定・今までどおり): 毎フレーム描く
   * - `"demand"`: 絵が変わりうる時だけ描き、止まっている間は requestAnimationFrame を回さない。
   *   描くのは board と applyChanges で盤面が変わった時・`topDown`・`placementY` の変化、ビューア上の操作
   *   (ドラッグ・ピンチ・wheel)、canvas の大きさの変化、`viewRef` の `invalidate()` を呼んだ時。
   *   **`cameraInputRef` は自分では見張らない**ので、値を入れたら `invalidate()` を呼ぶ。
   *   `cameraStateRef` は描いた時にだけ更新される
   */
  frameLoop?: 'always' | 'demand'
}

/**
 * unmount してから WebGL の文脈を手放すまでの待ち (ms)。
 * StrictMode の「外してすぐ付け直す」は同じ canvas を使い回すので、その間は手放さない
 */
const CONTEXT_RELEASE_DELAY_MS = 0

// ─── コンポーネント ────────────────────────────────────────────────────────────

export function IsometricView({
  board,
  preloadNames,
  pickRole,
  topDown = false,
  onBlockClick,
  placementY,
  cameraStateRef,
  panMode = false,
  cameraInputRef,
  onReady,
  viewRef,
  interactive = true,
  resourcePackUrl,
  frameLoop = 'always',
}: IsometricViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  // init effect は資源が変わった時だけ張り直すため、最新の onReady を ref 経由で読む
  const onReadyRef = useRef(onReady)
  onReadyRef.current = onReady
  const pickRoleRef = useRef<PickRoleFn>(pickRole ?? NO_TARGETS)
  pickRoleRef.current = pickRole ?? NO_TARGETS
  const rendererRef = useRef<StructureRenderer | null>(null)

  // ─── 描く盤面 (#11) ────────────────────────────────────────────────────────
  // React と WebGL の外のクラスに持つ。盤面の差し替え・applyChanges・読み込み中の変化を全部ここへ当てる
  const [model] = useState(() => new BoardModel(board))
  /** 最後に model へ当てた board。同じ物を渡し直されたら比べない */
  const appliedBoardRef = useRef(board)

  // ─── 描くタイミング (redstone-sim#434) ─────────────────────────────────────
  /** 予約済みの requestAnimationFrame の ID。0 = 予約なし (止まっている) */
  const animFrameRef = useRef<number>(0)
  /** 今の文脈で 1 フレーム描く関数。初期化が済むまでと、外した後は null */
  const drawRef = useRef<(() => void) | null>(null)
  const frameLoopRef = useRef(frameLoop)
  frameLoopRef.current = frameLoop
  /**
   * 次のフレームで描く。**もう予約があれば何もしない**ので、毎フレーム描く `"always"` では
   * どこから呼んでも今までと同じ (1 フレームに 1 回)。`"demand"` ではこれを呼んだ時だけ描く
   */
  const requestDraw = useCallback(() => {
    if (animFrameRef.current !== 0 || !drawRef.current) return
    animFrameRef.current = requestAnimationFrame(() => {
      animFrameRef.current = 0
      drawRef.current?.()
    })
  }, [])
  /** 最後に寄りを合わせたときの回路サイズ ("x,y,z")。変わったら合わせ直す (redstone-sim#238) */
  const fittedSizeRef = useRef<string>('')
  /**
   * 外部 (デモの setCamera 等) がカメラを指定したか (redstone-sim#238)。
   *
   * **自動フィットはテクスチャ読み込みを待ってから走る**ので、外から距離を指定しても
   * あとから上書きされて効かなかった。一度でも外部指定があったら自動フィットは譲る
   */
  const externalCameraRef = useRef(false)

  /** 構造の大きさ (構造ローカル。範囲の各軸の数) */
  const sizeOf = useCallback(() => model.structure.getSize() as [number, number, number], [model])

  /** 引ける上限。**回路の大きさに追従させる** — 固定 200 だと大きい回路で引ききれない (redstone-sim#238) */
  const zoomLimit = (): number => (rendererRef.current ? maxZoomOut(sizeOf()) : 200)

  /** 2D の層の y を構造ローカルにする。省いた時は下から 2 段目 (移す元の既定の 1 と同じ) */
  const localPlacementY = useCallback(
    () => (placementY === undefined ? 1 : placementY - model.min[1]),
    [placementY, model],
  )

  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')
  /** テクスチャがどこまで取れたか (redstone-sim#395)。full 以外なら盤面の左下に一言出す */
  const [assets, setAssets] = useState<AssetState>('full')
  const [loadingMsg, setLoadingMsg] = useState('Loading textures...')
  const [errorMsg, setErrorMsg] = useState('')

  const isPainting = useRef(false)
  const [isPanning, setIsPanning] = useState(false)

  const {
    cameraRef,
    onMouseDown, onMouseMove, onMouseUp,
    onRightMouseDown, onRightMouseMove, onRightMouseUp,
    onWheel3D,
    getPanScale,
  } = useCamera(topDown)

  /** **大きさが変わったら寄り直す** (redstone-sim#238)。外から指定されていたらそちらを尊重する (上書きしない) */
  const fitToSize = useCallback(() => {
    const size = sizeOf()
    const key = size.join(',')
    if (key === fittedSizeRef.current) return
    fittedSizeRef.current = key
    if (externalCameraRef.current) return
    const canvas = canvasRef.current
    cameraRef.current.distance = fitDistance(size, {
      aspect: canvas ? canvas.clientWidth / Math.max(1, canvas.clientHeight) : 1,
    })
  }, [sizeOf, cameraRef])

  // useCamera が topDown の切り替えでカメラを置き直す (この effect はその後に走る)。
  // placementY は 2D の当たり判定にしか使わないが、呼び出し側が層の絵と一緒に変えるので拾っておく。
  // frameLoop を "always" に戻した時は、ここから毎フレームの繰り返しが再開する
  useEffect(() => { requestDraw() }, [topDown, placementY, frameLoop, requestDraw])

  // canvas の大きさは描く時に合わせている。"demand" では大きさが変わった時に描き直す
  // ("always" では予約が常にあるので何も起きない)
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => requestDraw())
    ro.observe(canvas)
    return () => ro.disconnect()
  }, [requestDraw])

  // タブが裏から戻った時も描き直す。裏に回っている間に絵を捨てるブラウザでは、"demand" だと
  // 次の操作まで枠が空のままになる ("always" では予約が常にあるので何も起きない)
  useEffect(() => {
    const onVisible = () => { if (!document.hidden) requestDraw() }
    document.addEventListener('visibilitychange', onVisible)
    return () => document.removeEventListener('visibilitychange', onVisible)
  }, [requestDraw])

  // ─── WebGL の文脈を手放す (redstone-sim#434) ───────────────────────────────
  // canvas を DOM から外しても、文脈は GC されるまで残る。ブラウザの上限 (Chrome は 16) を
  // 越えると古い文脈から失われて警告が出るので、unmount で WEBGL_lose_context を呼んで返す。
  // 資源の作り直し (名前が増えた・resourcePackUrl) では同じ文脈を使い続けるので、ここは unmount の時だけ。
  // StrictMode は同じ canvas のまま「外す → 付ける」をするので、タイマーで 1 拍置き、
  // 付け直されたら取り消す
  const releaseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    if (releaseTimerRef.current !== null) {
      clearTimeout(releaseTimerRef.current)
      releaseTimerRef.current = null
    }
    const canvas = canvasRef.current
    return () => {
      if (!canvas) return
      releaseTimerRef.current = setTimeout(() => {
        releaseTimerRef.current = null
        // 取れなかった (WebGL 非対応) なら null。別の種類の文脈を作ってしまわないよう 'webgl' だけ見る
        canvas.getContext('webgl')?.getExtension('WEBGL_lose_context')?.loseContext()
      }, CONTEXT_RELEASE_DELAY_MS)
    }
  }, [])

  // ─── 資源を作る名前 (redstone-sim#234 → #11) ──────────────────────────────
  // preloadNames と、盤面と変化に出た名前を合わせる。名前は増えるだけで、知らない名前が出た時だけ
  // resourceKey を動かし、下の初期化を作り直す (渡さないと、そのブロックは**エラーも出さずに消えて見える**)
  const preloadRef = useRef<readonly string[]>(preloadNames ?? [])
  preloadRef.current = preloadNames ?? []
  const [initialNames] = useState(() => model.resourceNames(preloadNames ?? []))
  const resourceNamesRef = useRef<readonly string[]>(initialNames)
  const [resourceKey, setResourceKey] = useState(() => initialNames.join(' '))

  /** 名前が増えたら資源を作り直す。作り直すなら true */
  const syncNames = useCallback((): boolean => {
    const merged = model.resourceNames([...resourceNamesRef.current, ...preloadRef.current])
    // 今の名前を全部含めて合わせたので、増えたかは数で分かる
    if (merged.length === resourceNamesRef.current.length) return false
    resourceNamesRef.current = merged
    setResourceKey(merged.join(' '))
    return true
  }, [model])

  useEffect(() => { syncNames() }, [preloadNames, syncNames])

  // ─── 描く盤面の変化を絵へ当てる (#11) ──────────────────────────────────────
  /**
   * BoardModel が返した「何を作り直すか」に従う。
   * - 知らない名前が出た: 資源を作り直す (作り直した時に、その時の盤面から構造を作る)
   * - 読み込み中: 何もしない (準備ができた時に、その時の盤面から構造を作る)
   * - 範囲が変わった: setStructure (グリッドも作り直す) と寄りの合わせ直し
   * - それ以外: 変わったマスのチャンク (と、面で接する隣のチャンク) だけ。違いが多ければ全部
   */
  const handleUpdate = useCallback((u: ModelUpdate) => {
    if (u.newNames.length > 0 && syncNames()) return
    const renderer = rendererRef.current
    if (!renderer || u.redraw.kind === 'none') return
    if (u.redraw.kind === 'chunks') {
      renderer.updateStructureBuffers(u.redraw.chunks.map(c => [c[0], c[1], c[2]]))
    } else if (u.redraw.resized) {
      renderer.setStructure(model.structure)
      fitToSize()
    } else {
      renderer.updateStructureBuffers()
    }
    requestDraw()
  }, [model, syncNames, fitToSize, requestDraw])

  useEffect(() => {
    if (appliedBoardRef.current === board) return
    appliedBoardRef.current = board
    handleUpdate(model.setBoard(board))
  }, [board, model, handleUpdate])

  const applyChanges = useCallback((changes: Iterable<CellChange>): ApplyResult => {
    const u = model.applyChanges(changes)
    handleUpdate(u)
    return { changed: u.changed, outside: u.outside }
  }, [model, handleUpdate])

  const stateAt = useCallback((pos: Pos3) => model.stateAt(pos[0], pos[1], pos[2]), [model])

  // ─── ピンチ状態 ────────────────────────────────────────────────────────────
  const activePointers = useRef<Map<number, {x: number; y: number}>>(new Map())
  const lastPinchDist = useRef<number | null>(null)
  const lastPinchMid  = useRef<{x: number; y: number} | null>(null)

  // ─── 3D の 1 回の操作 (redstone-sim#397) ───────────────────────────────────
  /**
   * 押してから離すまで。**押した時点ではまだ何もしない**。
   * 動いた量がしきい値を越えたらドラッグ (カメラ回転)、越えずに離したらクリック・タップ。
   *  - press: 押したまま、しきい値の内側
   *  - drag:  しきい値を越えた。カメラを回している
   *  - pinch: 2 本目の指が触れた。指を全部離すまで何もしない
   */
  const gestureRef = useRef<{
    id: number
    x: number
    y: number
    /** 押した時刻 (event.timeStamp) */
    t: number
    touch: boolean
    /** クリックになりうるボタンか (左ボタン / 1 本目の指) */
    canTap: boolean
    phase: 'press' | 'drag' | 'pinch'
  } | null>(null)
  /** マウスが押せる素子の上にあるか (カーソルを pointer にする)。変わったときだけ state を変える */
  const hoverTargetRef = useRef(false)
  const [hoverTarget, setHoverTarget] = useState(false)

  // ─── ヒットテスト (2D モード) ──────────────────────────────────────────────

  /** 画面の点の、今の層のマス。**世界の座標** (構造ローカルに範囲の最小を足す) */
  const hitBlock = useCallback(
    (clientX: number, clientY: number): Pos3 | null => {
      const canvas = canvasRef.current
      if (!canvas || !rendererRef.current) return null
      const rect = canvas.getBoundingClientRect()
      const cam = cameraRef.current
      const local = canvasPixelToBlock(
        clientX - rect.left,
        clientY - rect.top,
        canvas.clientWidth,
        canvas.clientHeight,
        sizeOf(),
        cam.distance,
        localPlacementY(),
        cam.panX,
        cam.panZ,
      )
      if (!local) return null
      const min = model.min
      return [local[0] + min[0], local[1] + min[1], local[2] + min[2]]
    },
    [cameraRef, sizeOf, localPlacementY, model],
  )

  // ─── ピック (3D モード, redstone-sim#397) ──────────────────────────────────

  /**
   * 今描いている絵の行列と盤面の大きさ・原点。ピックと blockToClient が使う。描画の準備前は null。
   * 盤面は描く盤面 (model) から引く。絵とは同じ呼び出しの中で一緒に直すので、ずれない
   */
  const currentView = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas || !rendererRef.current) return null
    const rect = canvas.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0 || canvas.clientHeight <= 0) return null
    const size = sizeOf()
    return {
      left: rect.left,
      top: rect.top,
      // 画面の点 ↔ NDC は表示された大きさで、画面比は deepslate と同じ clientWidth/Height で
      w: rect.width,
      h: rect.height,
      size,
      min: model.min,
      vm: viewMatrixOf(cameraRef.current, size),
      pm: projMatrixOf(canvas.clientWidth / canvas.clientHeight),
    }
  }, [cameraRef, sizeOf, model])

  /** 画面の点 (clientX/Y) の光線で、最初に当たる押せる素子。座標は世界の座標 */
  const pick3D = useCallback(
    (clientX: number, clientY: number): Pos3 | null => {
      const view = currentView()
      if (!view) return null
      const ray = canvasRay(clientX - view.left, clientY - view.top, view.w, view.h, view.vm, view.pm)
      if (!ray) return null
      // 役は描く盤面の文字列で決める。空のマスでは pickRole を呼ばない
      const hit = pickCell(ray, view.size, (x, y, z) => {
        const state = model.stateAtLocal(x, y, z)
        return state === null ? null : pickRoleRef.current(state)
      })
      const { min } = view
      return hit ? [hit[0] + min[0], hit[1] + min[1], hit[2] + min[2]] : null
    },
    [currentView, model],
  )

  const blockToClient = useCallback(
    (pos: Pos3): { x: number; y: number } | null => {
      const view = currentView()
      if (!view) return null
      const { min } = view
      const p = [pos[0] - min[0] + 0.5, pos[1] - min[1] + 0.5, pos[2] - min[2] + 0.5]
      const c = pointToCanvas(p, view.w, view.h, view.vm, view.pm)
      return c ? { x: view.left + c.x, y: view.top + c.y } : null
    },
    [currentView],
  )

  /** クリック・タップした点のピック。タッチは外れても周り (半径 6px と 12px) を探す */
  const pickForTap = useCallback(
    (clientX: number, clientY: number, touch: boolean): Pos3 | null =>
      pickNearest(clientX, clientY, pick3D, blockToClient, touch ? TOUCH_PICK_RADII : []),
    [pick3D, blockToClient],
  )

  const updateHover = useCallback(
    (clientX: number, clientY: number) => {
      const over = !!onBlockClick && pick3D(clientX, clientY) !== null
      if (over === hoverTargetRef.current) return
      hoverTargetRef.current = over
      setHoverTarget(over)
    },
    [onBlockClick, pick3D],
  )

  useEffect(() => {
    if (!viewRef) return
    const handle: IsometricViewHandle = { blockToClient, invalidate: requestDraw, applyChanges, stateAt }
    viewRef.current = handle
    return () => {
      if (viewRef.current === handle) viewRef.current = null
    }
  }, [viewRef, blockToClient, requestDraw, applyChanges, stateAt])

  // ─── ポインターイベント ────────────────────────────────────────────────────

  const isPanButton = (button: number) => button === 2 || button === 1
  const isPanDrag = (buttons: number) => (buttons & 2) !== 0 || (buttons & 4) !== 0

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (!interactive) return
      activePointers.current.set(e.pointerId, {x: e.clientX, y: e.clientY})
      e.currentTarget.setPointerCapture(e.pointerId)

      // 2本指: ピンチ開始
      if (activePointers.current.size >= 2) {
        isPainting.current = false
        // 3D: ピンチになった操作はクリックにもドラッグにもしない (指を全部離すまで)
        if (gestureRef.current) gestureRef.current.phase = 'pinch'
        const pts = [...activePointers.current.values()]
        lastPinchDist.current = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y)
        lastPinchMid.current  = {x: (pts[0].x + pts[1].x) / 2, y: (pts[0].y + pts[1].y) / 2}
        return
      }

      // 1本指
      if (!topDown) {
        // 3D: 押した時点ではまだ回さない。動いた量で、ドラッグかクリックかを決める (redstone-sim#397)
        gestureRef.current = {
          id: e.pointerId,
          x: e.clientX,
          y: e.clientY,
          t: e.timeStamp,
          touch: e.pointerType === 'touch',
          canTap: e.button === 0 && e.isPrimary,
          phase: 'press',
        }
        return
      }
      if (isPanButton(e.button) || (panMode && e.button === 0)) {
        onRightMouseDown(e as unknown as React.MouseEvent, canvasRef.current)
        setIsPanning(true)
        return
      }
      if (!onBlockClick) return
      isPainting.current = true
      const pos = hitBlock(e.clientX, e.clientY)
      if (pos) onBlockClick(pos, e.button === 2 ? 'right' : 'left')
    },
    [interactive, topDown, panMode, onBlockClick, hitBlock, onRightMouseDown],
  )

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!interactive) return
      // カメラが動くのは押している間だけ。"demand" のために描く予約を入れる ("always" では何も起きない)
      if (e.buttons !== 0) requestDraw()
      activePointers.current.set(e.pointerId, {x: e.clientX, y: e.clientY})

      // 2本指: ピンチズーム + パン
      if (activePointers.current.size >= 2) {
        const pts = [...activePointers.current.values()]
        const dist = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y)
        const mid  = {x: (pts[0].x + pts[1].x) / 2, y: (pts[0].y + pts[1].y) / 2}
        const cam  = cameraRef.current

        if (lastPinchDist.current !== null && lastPinchMid.current !== null) {
          const scale   = lastPinchDist.current / dist
          const newDist = Math.max(3, Math.min(zoomLimit(), cam.distance * scale))

          if (topDown && rendererRef.current) {
            const canvas = canvasRef.current
            if (canvas) {
              // zoom-to-cursor（ピンチ中点を基準）。層の y は構造ローカル
              const rect   = canvas.getBoundingClientRect()
              const px     = mid.x - rect.left
              const py     = mid.y - rect.top
              const w      = canvas.clientWidth
              const h      = canvas.clientHeight
              const [sx, sy, sz] = sizeOf()
              const layerY = localPlacementY()
              const aspect = w / h
              const depth  = cam.distance + sy / 2 - layerY
              const ndcX   = (2 * px / w) - 1
              const ndcY   = 1 - (2 * py / h)
              const wx0    = ndcX * aspect * depth / FOV_F + sx / 2 + cam.panX
              const wz0    = sz / 2 - ndcY * depth / FOV_F + cam.panZ
              cam.distance = newDist
              const nd     = newDist + sy / 2 - layerY
              cam.panX    += wx0 - (ndcX * aspect * nd / FOV_F + sx / 2 + cam.panX)
              cam.panZ    += wz0 - (sz / 2 - ndcY * nd / FOV_F + cam.panZ)

              // 2本指パン（中点移動）
              const panScale = getPanScale(h)
              cam.panX -= (mid.x - lastPinchMid.current.x) * panScale
              cam.panZ -= (mid.y - lastPinchMid.current.y) * panScale
            }
          } else {
            cam.distance = newDist
          }
        }

        lastPinchDist.current = dist
        lastPinchMid.current  = mid
        return
      }

      // 1本指
      if (!topDown) {
        const g = gestureRef.current
        if (!g) {
          // 押していないマウス: 押せる素子の上ならカーソルを変える
          if (e.pointerType === 'mouse') updateHover(e.clientX, e.clientY)
          return
        }
        if (g.id !== e.pointerId || g.phase === 'pinch') return
        if (g.phase === 'press') {
          const slop = g.touch ? CLICK_SLOP_TOUCH : CLICK_SLOP_MOUSE
          if (Math.hypot(e.clientX - g.x, e.clientY - g.y) <= slop) return
          // しきい値を越えた: ここから回し始める。越えた位置を起点にするので絵は跳ばない
          g.phase = 'drag'
          setIsPanning(true)
          onMouseDown(e as unknown as React.MouseEvent)
          return
        }
        onMouseMove(e as unknown as React.MouseEvent)
        return
      }
      if (isPanDrag(e.buttons) || (panMode && (e.buttons & 1) !== 0)) {
        onRightMouseMove(e as unknown as React.MouseEvent, canvasRef.current)
        return
      }
      if (!isPainting.current || !onBlockClick) return
      const pos = hitBlock(e.clientX, e.clientY)
      if (pos) onBlockClick(pos, 'left')
    },
    // zoomLimit は描く盤面 (model) と rendererRef だけを読むので、deps に入れない
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [interactive, requestDraw, topDown, panMode, onBlockClick, hitBlock, onMouseDown, onMouseMove, onRightMouseMove, localPlacementY, sizeOf, cameraRef, getPanScale, updateHover],
  )

  /** pointerup に加えて pointerleave と pointercancel でも呼ぶ */
  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (!interactive) return
      // 3D: しきい値の内側で離したらクリック・タップ (redstone-sim#397)
      const g = gestureRef.current
      if (!topDown && g && g.id === e.pointerId) {
        gestureRef.current = null
        // pointerleave / pointercancel では押さない。押す位置は押した時の座標
        // (クリックではカメラが回らないので、押した時と離した時でカメラは同じ)
        if (e.type === 'pointerup' && g.phase === 'press' && g.canTap && onBlockClick
          && (!g.touch || e.timeStamp - g.t <= TAP_MAX_MS_TOUCH)) {
          const pos = pickForTap(g.x, g.y, g.touch)
          if (pos) onBlockClick(pos, 'left')
        }
      }
      activePointers.current.delete(e.pointerId)
      if (activePointers.current.size < 2) {
        lastPinchDist.current = null
        lastPinchMid.current  = null
      }
      isPainting.current = false
      setIsPanning(false)
      onRightMouseUp()
      if (!topDown) onMouseUp()
      // pointercancel の後や、タッチの pointerup 後の pointerleave では捕捉が外れている
      else if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
    },
    [interactive, topDown, onBlockClick, pickForTap, onMouseUp, onRightMouseUp],
  )

  // 操作の途中で interactive が false になったら、押しかけ・回しかけを全部捨てる (redstone-sim#434)
  useEffect(() => {
    if (interactive) return
    activePointers.current.clear()
    lastPinchDist.current = null
    lastPinchMid.current = null
    gestureRef.current = null
    isPainting.current = false
    hoverTargetRef.current = false
    setHoverTarget(false)
    setIsPanning(false)
    onMouseUp()
    onRightMouseUp()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [interactive])

  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      if (!interactive) return
      e.preventDefault()
      if (!topDown || !onBlockClick) return
      const pos = hitBlock(e.clientX, e.clientY)
      if (pos) onBlockClick(pos, 'right')
    },
    [interactive, topDown, onBlockClick, hitBlock],
  )

  // ─── ホイール (zoom-to-cursor 対応) ───────────────────────────────────────

  useEffect(() => {
    const canvas = canvasRef.current
    // interactive=false の間は listener を付けない (redstone-sim#434)。preventDefault しないのでページが流れる
    if (!canvas || !interactive) return

    const handleWheel = (e: WheelEvent) => {
      e.preventDefault()
      requestDraw()
      const cam = cameraRef.current

      if (!topDown || !rendererRef.current) {
        cam.distance = Math.max(3, Math.min(zoomLimit(), cam.distance + e.deltaY * 0.05))
        return
      }

      // zoom-to-cursor。層の y は構造ローカル
      const rect = canvas.getBoundingClientRect()
      const px = e.clientX - rect.left
      const py = e.clientY - rect.top
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      const [sx, sy, sz] = sizeOf()
      const layerY = localPlacementY()
      const aspect = w / h
      const oldDist = cam.distance

      const depth = oldDist + sy / 2 - layerY
      const ndcX = (2 * px / w) - 1
      const ndcY = 1 - (2 * py / h)
      const wx0 = ndcX * aspect * depth / FOV_F + sx / 2 + cam.panX
      const wz0 = sz / 2 - ndcY * depth / FOV_F + cam.panZ

      const factor = 1 + e.deltaY * 0.001
      cam.distance = Math.max(3, Math.min(zoomLimit(), oldDist * factor))

      const newDepth = cam.distance + sy / 2 - layerY
      const wx1 = ndcX * aspect * newDepth / FOV_F + sx / 2 + cam.panX
      const wz1 = sz / 2 - ndcY * newDepth / FOV_F + cam.panZ

      cam.panX += wx0 - wx1
      cam.panZ += wz0 - wz1
    }

    canvas.addEventListener('wheel', handleWheel, { passive: false })
    return () => canvas.removeEventListener('wheel', handleWheel)
    // zoomLimit は描く盤面 (model) と rendererRef だけを読むので、deps に入れない
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [topDown, localPlacementY, sizeOf, cameraRef, interactive, requestDraw])

  // ─── WebGL 初期化（マウント時 + 資源を作る名前が増えた時 + 資源の基点が変わった時） ─────

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return

    const gl = canvas.getContext('webgl')
    if (!gl) {
      setStatus('error')
      setErrorMsg('WebGL not supported')
      return
    }

    let cancelled = false
    let draw: (() => void) | null = null

    ;(async () => {
      try {
        setStatus('loading')
        setLoadingMsg('Loading textures...')

        // 取得先に届かなくても例外にせず、単色で描くリソースを返す (redstone-sim#395)
        const { resources, assets: builtAssets } = await buildResources(
          [...resourceNamesRef.current], resourcePackUrl)
        if (cancelled) return

        // **準備ができた時の盤面から構造を作る** (#11)。読み込み中に来た盤面の差し替えと変化も、
        // 描く盤面へ当ててあるので全部入る。移す元は effect が走った時の盤面で作っていたので、
        // 読み込み中の変化を落とし、アプリが onReady で描き直していた (redstone-sim#109)。
        //
        // **見えない mesh を作らせない** (redstone-sim#272)。deepslate の StructureRenderer は
        // options 未指定だと useInvisibleBlockBuffer が既定 true になり、
        // setStructure のたびに**盤面の空セル 1 個につき 12 本の線**を積んだ mesh を作り直す。
        // 描画するコードはこのリポジトリに 1 件も無いので、作った直後に捨てている。
        // 盤面の**体積**に比例するので、16x147x17 で 606ms / 64³ で 11.5 秒かかっていた (redstone-sim での実測)
        const renderer = new StructureRenderer(gl, model.structure, resources,
          { useInvisibleBlockBuffer: false })
        rendererRef.current = renderer

        // アトラステクスチャの MIN_FILTER を NEAREST に（UV ブリード防止）
        const internal = renderer as unknown as { atlasTexture: WebGLTexture | null }
        if (internal.atlasTexture) {
          gl.bindTexture(gl.TEXTURE_2D, internal.atlasTexture)
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
          gl.bindTexture(gl.TEXTURE_2D, null)
        }

        // **回路の大きさに合わせて寄る** (redstone-sim#238)。
        // 旧: max(幅,高さ,奥行) * 1.5 は視野角も画面比も見ていないため、
        // 147 段の回路が画面の高さの 3 割しか使わない糸のような柱になっていた。
        // 資源の作り直しでは大きさが変わらないので、寄りはそのまま (名前が増えるたびに寄り直さない)
        fitToSize()

        setAssets(builtAssets)
        setStatus('ready')
        let readyFired = false

        draw = () => {
          if (!rendererRef.current) return
          const cam = cameraRef.current

          // 外部からのカメラ上書き (fitCamera 等) を消費する
          if (cameraInputRef?.current) {
            const ci = cameraInputRef.current
            if (ci.distance !== undefined) {
              cam.distance = ci.distance
              externalCameraRef.current = true    // 以後の自動フィットは譲る
            }
            if (ci.panX !== undefined) cam.panX = ci.panX
            if (ci.panZ !== undefined) cam.panZ = ci.panZ
            if (ci.rotX !== undefined) cam.rotX = ci.rotX
            if (ci.rotY !== undefined) cam.rotY = ci.rotY
            cameraInputRef.current = null
          }

          const w = canvas.clientWidth
          const h = canvas.clientHeight
          if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w
            canvas.height = h
            renderer.setViewport(0, 0, w, h)
          }

          // ピック (redstone-sim#397) と同じ関数で作る。片方だけ変えると押した所と違うマスが切り替わる
          const vm = viewMatrixOf(cam, sizeOf())

          // カメラ状態を外部に公開（グリッドオーバーレイ等に使用）
          if (cameraStateRef) {
            cameraStateRef.current = { distance: cam.distance, panX: cam.panX, panZ: cam.panZ }
          }

          gl.clearColor(0.1, 0.1, 0.12, 1)
          gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)
          renderer.drawStructure(vm)
          renderer.drawGrid(vm)

          // 初回描画が済んだ時点で ready 通知 (デモの window.__demo.ready 用)
          if (!readyFired) {
            readyFired = true
            onReadyRef.current?.()
          }

          // "always" は毎フレーム描き続ける。"demand" はここで止まり、requestDraw を待つ (redstone-sim#434)
          if (frameLoopRef.current === 'always') requestDraw()
        }
        // 作り直し (資源の名前・resourcePackUrl) のときは、前の文脈の予約が残っていない
        // (cleanup で取り消している) ので、ここで必ず最初の 1 フレームが予約される
        drawRef.current = draw
        requestDraw()
      } catch (err: unknown) {
        if (cancelled) return
        const msg = err instanceof Error ? err.message : String(err)
        console.error('[IsometricView] init error', msg)
        setStatus('error')
        setErrorMsg(msg)
      }
    })()

    return () => {
      cancelled = true
      cancelAnimationFrame(animFrameRef.current)
      animFrameRef.current = 0
      if (drawRef.current === draw) drawRef.current = null
      rendererRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resourceKey, resourcePackUrl]) // 初回マウント時 + 資源の名前が増えたとき (redstone-sim#234) + 資源の基点が変わったとき (redstone-sim#434)

  // ─── レンダリング ──────────────────────────────────────────────────────────

  if (status === 'error') {
    return (
      <div data-viewer-status="error"
           style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: '100%', height: '100%', color: '#f87171', fontSize: 14 }}>
        Error: {errorMsg}
      </div>
    )
  }

  const editable = topDown && !!onBlockClick

  return (
    <div data-viewer-status={status} data-viewer-assets={assets}
         style={{ position: 'relative', width: '100%', height: '100%' }}>
      <canvas
        ref={canvasRef}
        style={{
          width: '100%',
          height: '100%',
          display: 'block',
          // 3D: 押せる素子の上で pointer、回している間は grabbing (redstone-sim#397)
          // interactive=false (redstone-sim#434): 操作を受けないので、カーソルは親に任せ、指のスクロールも通す
          cursor: !interactive ? undefined
            : isPanning ? 'grabbing' : topDown ? (panMode ? 'grab' : editable ? 'crosshair' : 'grab')
            : (hoverTarget && onBlockClick ? 'pointer' : 'grab'),
          touchAction: interactive ? 'none' : 'auto',
        }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerLeave={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onWheel={topDown || !interactive ? undefined : onWheel3D}
        onContextMenu={handleContextMenu}
      />
      {status === 'loading' && (
        <div style={{
          position: 'absolute', inset: 0,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: 'rgba(23,23,25,0.8)', color: '#a3a3a3', fontSize: 14,
        }}>
          {loadingMsg}
        </div>
      )}
      {/* 取得先に届かず単色で描いているとき (redstone-sim#395)。onReady はいつもどおり呼ぶ */}
      {status === 'ready' && assets !== 'full' && (
        <div data-testid="viewer-assets-note" style={{
          position: 'absolute', left: 8, bottom: 8, pointerEvents: 'none',
          padding: '2px 6px', background: 'rgba(23,23,25,0.8)', color: '#a3a3a3', fontSize: 11,
        }}>
          {assets === 'solid'
            ? 'テクスチャを読み込めないため簡易表示中'
            : '一部のテクスチャを読み込めませんでした'}
        </div>
      )}
    </div>
  )
}
