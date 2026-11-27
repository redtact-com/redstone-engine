/**
 * wasm の窓口 (C ABI) の形。Rust 側は crates/rse-wasm、表と決まりは docs/wasm-abi.md にある。
 *
 * ここの値 (関数・記録の並び・状態コードのビット・種類 / 区間 / 型の番号) は境界を越えるので、
 * Rust 側と同じ値で固定する。変えたら両方の ABI_VERSION を上げる。
 */

/** 窓口の版。読み込んだ wasm の rse_abi_version と合わなければ断る */
export const ABI_VERSION = 1

/** wasm が書き出す物。ポインタは線形メモリの番地 (i32 で来るので `>>> 0` で読む) */
export interface RseExports {
  readonly memory: WebAssembly.Memory
  rse_abi_version(): number
  rse_scratch(len: number): number
  rse_world_new(): number
  rse_world_free(handle: number): number
  rse_resolve(ptr: number, len: number, out: number): number
  rse_load(handle: number, recs: number, n: number, mode: number): number
  rse_step(handle: number, n: number): number
  rse_input(handle: number, kind: number, section: number, x: number, y: number, z: number, arg: number): number
  rse_changes(handle: number, lenOut: number): number
}

/** 書き出す関数 (7 関数 + 作業領域の 1 + 版の 1) */
export const EXPORTED_FUNCTIONS = [
  'rse_abi_version',
  'rse_scratch',
  'rse_world_new',
  'rse_world_free',
  'rse_resolve',
  'rse_load',
  'rse_step',
  'rse_input',
  'rse_changes',
] as const

/** マスの記録 (x, y, z, state。どれも 32 ビット) の i32 の数とバイト数。読み込みと変化で同じ形 */
export const CELL_WORDS = 4
export const CELL_BYTES = 16

/** 読み込みの型 (19 §4.2)。rebuild は席だけで、UNIMPLEMENTED を返す */
export const LOAD_MODE = { settled: 0, rebuild: 1 } as const
export type LoadMode = keyof typeof LOAD_MODE

/** 入力の種類。setBlock は席だけで、UNIMPLEMENTED を返す */
export const INPUT_KIND = { use: 0, setBlock: 1 } as const
export type InputKind = keyof typeof INPUT_KIND

/** 入力の区間の印 (19 §4.1)。値は rse-core の InputSection と同じ。順は変えない (呼んだ順に適用する) */
export const SECTION = { B5: 0, C: 1, A: 2, B1: 3 } as const
export type Section = keyof typeof SECTION

/** エンジンの状態コード (下の 16 ビット。rse-core の Status と同じ値) */
export const ENGINE_STATUS = {
  CHAIN_LIMIT: 1 << 0,
  TICK_CARRYOVER: 1 << 1,
  LOADED_ORDER_TIE: 1 << 2,
  TREE_BIN: 1 << 3,
  UNIMPLEMENTED: 1 << 4,
  SAFETY: 1 << 5,
} as const

/** 窓口の状態コード (上の 16 ビット。crates/rse-wasm の status と同じ値) */
export const ABI_STATUS = {
  HANDLE: 1 << 16,
  ARGUMENT: 1 << 17,
  OUT_OF_WORLD: 1 << 18,
  UNKNOWN_BLOCK: 1 << 19,
  UNKNOWN_PROPERTY: 1 << 20,
  UNKNOWN_VALUE: 1 << 21,
  SYNTAX: 1 << 22,
} as const

export const STATUS = { ...ENGINE_STATUS, ...ABI_STATUS } as const
export type StatusName = keyof typeof STATUS

/** 下の 16 ビット (エンジンの状態コード) */
export const ENGINE_STATUS_MASK = 0xffff

/** 状態コードの文 (19 §2.5 の表) */
export const STATUS_TEXT: Record<StatusName, string> = {
  CHAIN_LIMIT: '近隣更新の連鎖が上限に達し、新しい分を捨てた',
  TICK_CARRYOVER: '予約 tick が 1 tick の上限に達し、持ち越した',
  LOADED_ORDER_TIE: '別のチャンクの予約どうしが同点になり、順が一意でない',
  TREE_BIN: 'HashMap の写しが木構造の条件に達した (実装していない)',
  UNIMPLEMENTED: '未実装の経路を通った',
  SAFETY: '安全弁が働き、実機と違う結果になった',
  HANDLE: '世界のハンドルが無い (作っていない・もう捨てた)',
  ARGUMENT: '引数が決まりの外 (作業領域の外・揃っていない・番号が無い・StateId が表の外)',
  OUT_OF_WORLD: '置けない (高さの外・格子の枠の外)',
  UNKNOWN_BLOCK: '状態表に無いブロックの名前',
  UNKNOWN_PROPERTY: 'そのブロックに無いプロパティ',
  UNKNOWN_VALUE: 'そのプロパティに無い値',
  SYNTAX: '名前の形が崩れている',
}

/** 立っているビットの名前 (ビットの番号の順)。表に無いビットは `bit N` と書く */
export function statusNames(bits: number): string[] {
  const out: string[] = []
  for (let n = 0; n < 32; n++) {
    const bit = (1 << n) >>> 0
    if ((bits >>> 0) & bit) {
      const name = (Object.keys(STATUS) as StatusName[]).find(k => STATUS[k] === bit)
      out.push(name ?? `bit ${n}`)
    }
  }
  return out
}

/** 窓口が状態コードを返して断った時の例外 */
export class EngineError extends Error {
  readonly status: number

  constructor(what: string, status: number) {
    super(`${what}: ${statusNames(status).join(' | ')}`)
    this.name = 'EngineError'
    this.status = status >>> 0
  }
}

/**
 * wasm の中で trap (WebAssembly.RuntimeError) が起きた後の例外。
 *
 * release の wasm は panic = abort なので、中の panic (今の経路では割り当ての失敗) は trap になる。
 * trap の後はインスタンスの中の表が借りたまま戻らず、どの関数も trap し続ける。
 * インスタンスが壊れた印なので、Engine ごと捨てて読み直す (docs/wasm-abi.md §4)。
 */
export class EngineStoppedError extends Error {
  constructor(what: string, cause: unknown) {
    const reason = cause instanceof Error ? cause.message : String(cause)
    super(`${what}: wasm の中で trap が起きて、この Engine は止まった。Engine ごと捨てて読み直す (${reason})`, { cause })
    this.name = 'EngineStoppedError'
  }
}
