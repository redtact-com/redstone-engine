/**
 * 世界 1 つのハンドル (RustWorld) と、変化の窓 (ChangeWindow)。
 *
 * - 置く (load)・進める (step)・入力 (use・input)・変化を読む (changes)・捨てる (dispose)。
 * - 窓口の状態コード (上の 16 ビット) は使い方の誤りなので、EngineError を投げる。
 *   エンジンの状態コード (下の 16 ビット) は「実機と違う結果になりうる」の知らせなので、値で返す。
 *   どちらも黙って捨てない (19 §2.5)。
 * - 表に無い名前 (型・種類・区間) と、範囲の外の数は、wasm に渡す前に RangeError で断る。
 * - 捨てた世界の操作は、wasm を呼ばずに EngineError (HANDLE) を投げる。
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
  SECTION,
  type Section,
} from './abi'
import type { Core } from './engine'

/** 1 マス */
export interface Cell {
  readonly x: number
  readonly y: number
  readonly z: number
  readonly state: number
}

interface Held {
  readonly core: Core
  readonly handle: number
}

/**
 * 安全網: dispose を忘れた世界が回収された時に、その世界を捨てる (19 §6.3)。呼ぶ所は dispose が本筋。
 *
 * - 呼び出しは Core を通す (epoch が進むので、ほかの世界の変化の窓も古くなる。rse_world_free も
 *   線形メモリを伸ばしうる)。
 * - 止まった Engine では呼ばない。回収の時の例外は受ける人がいないので、外へ出さない。
 * @internal テストから直に呼ぶために書き出す
 */
export function releaseLeaked({ core, handle }: Held): void {
  if (core.stopped) return
  try {
    core.call('安全網の dispose', x => x.rse_world_free(handle))
  } catch {
    // 回収の時に投げても受ける所が無い (Node ではプロセスが終わる)。
  }
}

const finalizer = new FinalizationRegistry<Held>(releaseLeaked)

function int32(v: number, what: string): number {
  if (!Number.isInteger(v) || v < -0x80000000 || v > 0x7fffffff) throw new RangeError(`${what} が 32 ビットの整数でない: ${v}`)
  return v
}

/** 表 (LOAD_MODE ほか) の自分の鍵だけを引く。継いだ鍵 (toString など) と表に無い名前は断る */
function pick<T extends Record<string, number>>(table: T, key: unknown, what: string): number {
  if (typeof key !== 'string' || !Object.hasOwn(table, key)) throw new RangeError(`${what}でない: ${String(key)}`)
  return table[key as keyof T]
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
  readonly #core: Core
  readonly #handle: number
  #disposed = false

  /** Engine.createWorld から作る */
  constructor(core: Core, handle: number) {
    this.#core = core
    this.#handle = handle
    finalizer.register(this, { core, handle }, this)
  }

  get handle(): number {
    return this.#handle
  }

  get disposed(): boolean {
    return this.#disposed
  }

  /** 捨てた世界なら、wasm を呼ばずに投げる (同じハンドルの席が使い回されていても、別の世界に当てない) */
  #live(what: string): void {
    if (this.#disposed) throw new EngineError(`${what} (捨てた世界)`, ABI_STATUS.HANDLE)
  }

  /** 窓口の状態コードなら投げ、エンジンの状態コードを返す */
  #check(what: string, status: number): number {
    const s = status >>> 0
    if ((s & ~ENGINE_STATUS_MASK) !== 0) throw new EngineError(what, s)
    return s
  }

  /**
   * マスを置く。settled (既定) は更新を出さずに置く (19 §4.2 の「そのまま」)。rebuild は席だけで、
   * UNIMPLEMENTED を返す。StateId が表の外か、高さ・格子の枠の外のマスがあれば、1 つも置かずに投げる。
   * 置いたマスは変化の列に載る。
   *
   * @param cells マスの列か、(x, y, z, state) を詰めた Int32Array。線形メモリの上の配列 (変化の窓の data) も渡せる
   * @returns エンジンの状態コード
   */
  load(cells: readonly Cell[] | Int32Array, mode: LoadMode = 'settled'): number {
    this.#live('load')
    const code = pick(LOAD_MODE, mode, '読み込みの型')
    let words = cells instanceof Int32Array ? cells : pack(cells)
    if (words.length % CELL_WORDS !== 0) throw new RangeError(`マスの記録は ${CELL_WORDS} 個ずつ`)
    // 線形メモリの上の配列は、作業領域を広げると切り離されうるので、先に写す。
    if (words.buffer === this.#core.buffer) words = words.slice()
    const n = words.length / CELL_WORDS
    const base = this.#core.scratch(n * CELL_BYTES, 'load')
    new Int32Array(this.#core.buffer, base, words.length).set(words)
    return this.#check('load', this.#core.call('load', x => x.rse_load(this.#handle, base, n, code)))
  }

  /** n tick 進め、その間に立ったエンジンの状態コードを返す */
  step(n = 1): number {
    this.#live('step')
    if (!Number.isInteger(n) || n < 0 || n > 0xffffffff) throw new RangeError(`tick の数でない: ${n}`)
    return this.#check('step', this.#core.call('step', x => x.rse_step(this.#handle, n)))
  }

  /** 使う (右クリック)。既定の区間はプレイヤーの操作 (A。19 §4.1) */
  use(x: number, y: number, z: number, section: Section = 'A'): number {
    return this.input('use', section, x, y, z)
  }

  /**
   * tick の外の入力を 1 件入れる。戻った時には連鎖が回り切っている。区間の印は順を変えない。
   * @param arg setBlock は StateId | (strict << 16)。use は 0。0〜0xFFFFFFFF の整数
   * @returns エンジンの状態コード
   */
  input(kind: InputKind, section: Section, x: number, y: number, z: number, arg = 0): number {
    this.#live(`input (${String(kind)})`)
    const k = pick(INPUT_KIND, kind, '入力の種類')
    const s = pick(SECTION, section, '入力の区間')
    if (!Number.isInteger(arg) || arg < 0 || arg > 0xffffffff) throw new RangeError(`arg が 32 ビットの符号なし整数でない: ${arg}`)
    const pos = [int32(x, 'x'), int32(y, 'y'), int32(z, 'z')] as const
    const status = this.#core.call('input', raw => raw.rse_input(this.#handle, k, s, pos[0], pos[1], pos[2], arg))
    return this.#check(`input (${kind})`, status)
  }

  /**
   * 前に取り出した後の正味の変化 (19 §3.8)。線形メモリの窓で、写しを作らない。
   * 窓は、次に窓口を呼ぶ (どの世界でも) と使えなくなる。続けて使う値は、その前に読み出す。
   */
  changes(): ChangeWindow {
    this.#live('changes')
    const lenOut = this.#core.scratch(4, 'changes')
    const ptr = this.#core.call('changes', x => x.rse_changes(this.#handle, lenOut)) >>> 0
    // 件数の書き先は作業領域から取ったので、null はハンドルが無い時だけ (変化は取り出されていない)。
    if (ptr === 0) throw new EngineError('changes', ABI_STATUS.HANDLE)
    // rse_changes は列を伸ばすので、線形メモリが伸びたかもしれない。窓はここで作る。
    const buffer = this.#core.buffer
    const count = new Uint32Array(buffer, lenOut, 1)[0]
    return new ChangeWindow(this.#core, this, new Int32Array(buffer, ptr, count * CELL_WORDS), count)
  }

  /**
   * 世界を捨てる。2 回目からは何もしない。
   * 止まった Engine (trap の後) では、世界はインスタンスと一緒に失われているので、投げずに終える
   * (finally の中の dispose が、元の EngineStoppedError を隠さないように)。
   */
  dispose(): void {
    if (this.#disposed) return
    this.#disposed = true
    finalizer.unregister(this)
    if (this.#core.stopped) return
    this.#check('dispose', this.#core.call('dispose', x => x.rse_world_free(this.#handle)))
  }
}

/** 変化の列の窓。i 番目のマスを x(i)・y(i)・z(i)・state(i) で読む */
export class ChangeWindow {
  /** 件数 */
  readonly count: number
  /** 窓の世界。窓が生きている間は、世界を安全網に回収させない (回収されると列が解放される) */
  readonly world: RustWorld
  readonly #core: Core
  readonly #epoch: number
  readonly #data: Int32Array

  /** RustWorld.changes から作る */
  constructor(core: Core, world: RustWorld, data: Int32Array, count: number) {
    this.#core = core
    this.#epoch = core.epoch
    this.#data = data
    this.world = world
    this.count = count
  }

  /**
   * まだ読めるか。この後に窓口を呼んでいず、線形メモリも入れ替わっていない (伸びていない) こと。
   * epoch に頼らず buffer も比べるので、外から `memory.grow` を呼んで切り離された時も読まずに断る。
   */
  get valid(): boolean {
    return this.#core.epoch === this.#epoch && this.#data.buffer === this.#core.buffer
  }

  #live(): Int32Array {
    if (!this.valid) throw new Error('変化の窓は、次に窓口を呼んだ時点で使えなくなる。先に読み出す')
    return this.#data
  }

  /**
   * (x, y, z, state) が count 件並んだ、線形メモリの窓 (写しではない)。
   * 読むだけにし、持ち続けない (書いても書き出し済みの列が変わるだけで、エンジンの世界には届かない。
   * 次に窓口を呼ぶと、中身が変わるか切り離される)。
   */
  get data(): Int32Array {
    return this.#live()
  }

  #at(i: number, k: number): number {
    const data = this.#live()
    if (!Number.isInteger(i) || i < 0 || i >= this.count) throw new RangeError(`変化の添字が範囲の外: ${i} (件数 ${this.count})`)
    return data[i * CELL_WORDS + k]
  }

  x(i: number): number {
    return this.#at(i, 0)
  }

  y(i: number): number {
    return this.#at(i, 1)
  }

  z(i: number): number {
    return this.#at(i, 2)
  }

  state(i: number): number {
    return this.#at(i, 3)
  }
}
