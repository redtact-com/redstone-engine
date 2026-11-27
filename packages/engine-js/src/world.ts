/**
 * 世界 1 つのハンドル (RustWorld) と、変化の窓 (ChangeWindow)。
 *
 * - 置く (load)・進める (step)・入力 (use・input)・変化を読む (changes)・捨てる (dispose)。
 * - 窓口の状態コード (上の 16 ビット) は使い方の誤りなので、EngineError を投げる。
 *   エンジンの状態コード (下の 16 ビット) は「実機と違う結果になりうる」の知らせなので、値で返す。
 *   どちらも黙って捨てない (19 §2.5)。
 * - 盤面の写しは持たない。呼ぶ側が変化の窓で直す (fixture の実行器と、後のアプリ)。
 */

import {
  ABI_STATUS,
  CELL_BYTES,
  CELL_WORDS,
  ENGINE_STATUS_MASK,
  EngineError,
  INPUT_KIND,
  type InputKind,
  LOAD_MODE,
  type LoadMode,
  type RseExports,
  SECTION,
  type Section,
} from './abi'
import type { Engine } from './engine'

/** 1 マス */
export interface Cell {
  readonly x: number
  readonly y: number
  readonly z: number
  readonly state: number
}

/** dispose を忘れた世界を捨てる安全網 (19 §6.3)。呼ぶ所は dispose が本筋 */
const finalizer = new FinalizationRegistry<{ raw: RseExports; handle: number }>(({ raw, handle }) => {
  raw.rse_world_free(handle)
})

function int32(v: number, what: string): number {
  if (!Number.isInteger(v) || v < -0x80000000 || v > 0x7fffffff) throw new RangeError(`${what} が 32 ビットの整数でない: ${v}`)
  return v
}

/** マスの列を、マスの記録 (x, y, z, state) の並びに詰める */
function pack(cells: readonly Cell[]): Int32Array {
  const out = new Int32Array(cells.length * CELL_WORDS)
  cells.forEach((c, i) => {
    if (!Number.isInteger(c.state) || c.state < 0 || c.state > 0xffff) throw new RangeError(`StateId でない: ${c.state}`)
    out.set([int32(c.x, 'x'), int32(c.y, 'y'), int32(c.z, 'z'), c.state], i * CELL_WORDS)
  })
  return out
}

export class RustWorld {
  readonly #engine: Engine
  readonly #handle: number
  #disposed = false

  /** @internal Engine.createWorld から作る */
  constructor(engine: Engine, handle: number) {
    this.#engine = engine
    this.#handle = handle
    finalizer.register(this, { raw: engine.raw, handle }, this)
  }

  get handle(): number {
    return this.#handle
  }

  get disposed(): boolean {
    return this.#disposed
  }

  /** 窓口の状態コードなら投げ、エンジンの状態コードを返す */
  #check(what: string, status: number): number {
    const s = status >>> 0
    if ((s & ~ENGINE_STATUS_MASK) !== 0) throw new EngineError(what, s)
    return s
  }

  /**
   * マスを置く。settled (既定) は更新を出さずに置く (19 §4.2 の「そのまま」)。rebuild は席だけで、
   * UNIMPLEMENTED を返す。StateId が表の外か、高さの外のマスがあれば 1 つも置かずに投げる。
   * 格子の枠の外 (離れすぎた 2 点) で投げた時は、途中まで置いた世界が残るので、捨てて作り直す。
   * 置いたマスは変化の列に載る。
   *
   * @param cells マスの列か、(x, y, z, state) を詰めた Int32Array
   * @returns エンジンの状態コード
   */
  load(cells: readonly Cell[] | Int32Array, mode: LoadMode = 'settled'): number {
    const words = cells instanceof Int32Array ? cells : pack(cells)
    if (words.length % CELL_WORDS !== 0) throw new RangeError(`マスの記録は ${CELL_WORDS} 個ずつ`)
    const n = words.length / CELL_WORDS
    const base = this.#engine.scratch(n * CELL_BYTES)
    new Int32Array(this.#engine.buffer, base, words.length).set(words)
    return this.#check('load', this.#engine.call(x => x.rse_load(this.#handle, base, n, LOAD_MODE[mode])))
  }

  /** n tick 進め、その間に立ったエンジンの状態コードを返す */
  step(n = 1): number {
    if (!Number.isInteger(n) || n < 0 || n > 0xffffffff) throw new RangeError(`tick の数でない: ${n}`)
    return this.#check('step', this.#engine.call(x => x.rse_step(this.#handle, n)))
  }

  /** 使う (右クリック)。既定の区間はプレイヤーの操作 (A。19 §4.1) */
  use(x: number, y: number, z: number, section: Section = 'A'): number {
    return this.input('use', section, x, y, z)
  }

  /**
   * tick の外の入力を 1 件入れる。戻った時には連鎖が回り切っている。区間の印は順を変えない。
   * @param arg setBlock は StateId | (strict << 16)。use は 0
   * @returns エンジンの状態コード
   */
  input(kind: InputKind, section: Section, x: number, y: number, z: number, arg = 0): number {
    const status = this.#engine.call(raw =>
      raw.rse_input(this.#handle, INPUT_KIND[kind], SECTION[section], int32(x, 'x'), int32(y, 'y'), int32(z, 'z'), arg >>> 0),
    )
    return this.#check(`input (${kind})`, status)
  }

  /**
   * 前に取り出した後の正味の変化 (19 §3.8)。線形メモリの窓で、写しを作らない。
   * 窓は、次に窓口を呼ぶ (どの世界でも) と使えなくなる。続けて使う値は、その前に読み出す。
   */
  changes(): ChangeWindow {
    const lenOut = this.#engine.scratch(4)
    const ptr = this.#engine.call(x => x.rse_changes(this.#handle, lenOut)) >>> 0
    // rse_changes は列を伸ばすので、線形メモリが伸びたかもしれない。窓はここで作る。
    const buffer = this.#engine.buffer
    const count = new Uint32Array(buffer, lenOut, 1)[0]
    // 件数の書き先は作業領域から取ったので、null はハンドルが無い時だけ (変化は取り出されていない)。
    if (ptr === 0) throw new EngineError('changes', ABI_STATUS.HANDLE)
    return new ChangeWindow(this.#engine, new Int32Array(buffer, ptr, count * CELL_WORDS), count)
  }

  /** 世界を捨てる。2 回目からは何もしない */
  dispose(): void {
    if (this.#disposed) return
    this.#disposed = true
    finalizer.unregister(this)
    this.#check('dispose', this.#engine.call(x => x.rse_world_free(this.#handle)))
  }
}

/** 変化の列の窓。i 番目のマスを x(i)・y(i)・z(i)・state(i) で読む */
export class ChangeWindow {
  /** 件数 */
  readonly count: number
  readonly #engine: Engine
  readonly #epoch: number
  readonly #data: Int32Array

  /** @internal */
  constructor(engine: Engine, data: Int32Array, count: number) {
    this.#engine = engine
    this.#epoch = engine.epoch
    this.#data = data
    this.count = count
  }

  /** まだ読めるか (この後に窓口を呼んでいないか) */
  get valid(): boolean {
    return this.#engine.epoch === this.#epoch
  }

  #live(): Int32Array {
    if (!this.valid) throw new Error('変化の窓は、次に窓口を呼んだ時点で使えなくなる。先に読み出す')
    return this.#data
  }

  /** (x, y, z, state) が count 件並んだ、線形メモリの窓 (写しではない) */
  get data(): Int32Array {
    return this.#live()
  }

  x(i: number): number {
    return this.#live()[i * CELL_WORDS]
  }

  y(i: number): number {
    return this.#live()[i * CELL_WORDS + 1]
  }

  z(i: number): number {
    return this.#live()[i * CELL_WORDS + 2]
  }

  state(i: number): number {
    return this.#live()[i * CELL_WORDS + 3]
  }
}
