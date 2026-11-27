/**
 * wasm の読み込みと、名前の解決 (19 §2.5)。
 *
 * - wasm は import が 0 個なので、Node・ブラウザ・Worker で同じ `WebAssembly.instantiate(…, {})` で読める。
 *   `Engine.load` はバイト列か Response を受け、場所を知らない。ファイルから読むのは ./node.ts、
 *   URL から読むのは `loadEngineFromUrls` (ブラウザ。場所はアプリが決める)。
 * - インスタンス 1 つに世界をいくつも持てる (`createWorld`)。世界はハンドルで持つ。
 * - **線形メモリの窓 (型付き配列) は、窓口を呼ぶたびに作り直す**。窓口の関数は割り当てで線形メモリを
 *   伸ばすことがあり、伸びると前の ArrayBuffer は切り離されて (長さ 0) 読めなくなる。
 *   そこで、呼んだ後に `memory.buffer` から窓を作り、次に呼ぶ前に使い終える。
 * - 窓口の関数は、全部 Core を通して呼ぶ。Core は呼んだ回数 (epoch) を数え、trap の後は呼ばずに投げる。
 * - SharedArrayBuffer と Worker は使わない (19 §11 の 14)。
 */

import { ABI_STATUS, ABI_VERSION, EXPORTED_FUNCTIONS, EngineError, EngineStoppedError, type RseExports } from './abi'
import { StateTable } from './states'
import { RustWorld } from './world'

/** wasm の元。バイト列・コンパイル済みの Module・fetch の Response (か、その Promise) */
export type WasmSource = BufferSource | WebAssembly.Module | Response | PromiseLike<Response>

export type ResolveResult = { readonly ok: true; readonly state: number } | { readonly ok: false; readonly status: number }

const encoder = new TextEncoder()

/**
 * 1 つのインスタンスへの呼び出しの口。engine-js の中だけで使う (index.ts から出さない)。
 *
 * - 呼ぶたびに epoch を進める。変化の窓は、作った時の epoch と比べて古さを見る。
 * - wasm の中で trap (WebAssembly.RuntimeError) が起きたら止まった状態にし、以後は wasm を呼ばずに
 *   EngineStoppedError を投げる。trap の後は中の表の借用が戻らず、どの関数も trap し続けるため。
 */
export class Core {
  readonly #raw: RseExports
  #epoch = 0
  #stopped: unknown = undefined

  constructor(raw: RseExports) {
    this.#raw = raw
  }

  get epoch(): number {
    return this.#epoch
  }

  get stopped(): boolean {
    return this.#stopped !== undefined
  }

  get memory(): WebAssembly.Memory {
    return this.#raw.memory
  }

  /** 今の線形メモリ。窓口を呼んだ後は、必ず取り直す */
  get buffer(): ArrayBuffer {
    return this.#raw.memory.buffer
  }

  /** 止まっていたら投げる (wasm を呼ばない操作も、止まった Engine では断る) */
  live(what: string): void {
    if (this.#stopped !== undefined) throw new EngineStoppedError(what, this.#stopped)
  }

  /** 窓口の関数を呼ぶ。呼ぶ前に epoch を進めるので、前に作った変化の窓は使えなくなる */
  call<T>(what: string, f: (raw: RseExports) => T): T {
    this.live(what)
    this.#epoch++
    try {
      return f(this.#raw)
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) {
        this.#stopped = e
        throw new EngineStoppedError(what, e)
      }
      throw e
    }
  }

  /** 作業領域を len バイト以上に広げ、先頭の番地を返す。線形メモリが伸びうるので、窓はこの後に作る */
  scratch(len: number): number {
    const ptr = this.call('scratch', x => x.rse_scratch(len)) >>> 0
    if (ptr === 0) throw new RangeError(`作業領域を ${len} バイトに広げられない`)
    return ptr
  }
}

async function instantiate(source: WasmSource): Promise<WebAssembly.Instance> {
  const src = await source
  if (src instanceof WebAssembly.Module) return WebAssembly.instantiate(src, {})
  if (typeof Response !== 'undefined' && src instanceof Response) {
    if (!src.ok) throw new Error(`wasm を取れない: ${src.status} ${src.url}`)
    if (typeof WebAssembly.instantiateStreaming === 'function') {
      try {
        return (await WebAssembly.instantiateStreaming(src.clone(), {})).instance
      } catch {
        // MIME 型が application/wasm でない配信では読めないので、バイト列で読み直す。
      }
    }
    return (await WebAssembly.instantiate(await src.arrayBuffer(), {})).instance
  }
  return (await WebAssembly.instantiate(src as BufferSource, {})).instance
}

/** 書き出しを確かめる。版を先に見る (版の違う wasm は、関数の顔ぶれも違いうるので、版の違いとして断る) */
function exportsOf(exports: WebAssembly.Exports): RseExports {
  const x = exports as Record<string, unknown>
  if (typeof x.rse_abi_version !== 'function') throw new Error('wasm に rse_abi_version が無い (この窓口の wasm でない)')
  const version = (x.rse_abi_version as () => number)()
  if (version !== ABI_VERSION) throw new Error(`窓口の版が合わない: wasm は ${version}・engine-js は ${ABI_VERSION}`)
  const missing = EXPORTED_FUNCTIONS.filter(name => typeof x[name] !== 'function')
  if (missing.length > 0) throw new Error(`wasm に関数が無い: ${missing.join(', ')}`)
  if (!(x.memory instanceof WebAssembly.Memory)) throw new Error('wasm が memory を書き出していない')
  return x as unknown as RseExports
}

export class Engine {
  readonly states: StateTable
  readonly #core: Core
  readonly #cache = new Map<string, number>()

  private constructor(core: Core, states: StateTable) {
    this.#core = core
    this.states = states
  }

  /**
   * wasm と状態表の JSON から作る。窓口の版と、状態表が wasm と同じビルドの物かを確かめる。
   * 状態表は、rse-cli が書いた JSON (文字列か、読んだ値) か、作った StateTable。
   */
  static async load(wasm: WasmSource, states: unknown): Promise<Engine> {
    return Engine.fromExports((await instantiate(wasm)).exports, states)
  }

  /** インスタンスを自分で作った時に、その書き出しから作る。確かめは `load` と同じ */
  static fromExports(exports: WebAssembly.Exports, states: unknown): Engine {
    const engine = new Engine(new Core(exportsOf(exports)), states instanceof StateTable ? states : StateTable.parse(states))
    engine.#checkTable()
    return engine
  }

  /**
   * 状態表の JSON が古い (wasm と別のビルド) と、StateId の名前を取り違える。全部の状態の名前を wasm で
   * 解決し直して番号を比べ、ブロックの名前だけ (既定の状態) も比べる。性質のビットは比べない (docs/wasm-abi.md §6)。
   */
  #checkTable(): void {
    const mismatch = (name: string) =>
      new Error(`状態表の JSON が wasm と合わない (${name})。npm run build:engine で作り直す`)
    for (let id = 0; id < this.states.size; id++) {
      const got = this.tryResolve(this.states.name(id))
      if (!got.ok || got.state !== id) throw mismatch(this.states.name(id))
    }
    for (const b of this.states.blocks) {
      const got = this.tryResolve(b.name)
      if (!got.ok || got.state !== b.defaultState) throw mismatch(`${b.name} の既定の状態`)
    }
  }

  /** 窓口の版 */
  get abiVersion(): number {
    return ABI_VERSION
  }

  /** 窓口を呼んだ回数 (読むだけ)。変化の窓は、これが進むと使えなくなる */
  get epoch(): number {
    return this.#core.epoch
  }

  /** wasm の中で trap が起きて止まったか。止まった Engine は捨てて読み直す */
  get stopped(): boolean {
    return this.#core.stopped
  }

  /** 線形メモリ。中身は窓口を呼ぶと変わり、伸びると buffer が入れ替わる */
  get memory(): WebAssembly.Memory {
    return this.#core.memory
  }

  /** 世界を 1 つ作る。使い終えたら dispose する */
  createWorld(): RustWorld {
    const handle = this.#core.call('createWorld', x => x.rse_world_new()) >>> 0
    if (handle === 0) throw new EngineError('世界を作れない (席が尽きた)', ABI_STATUS.HANDLE)
    return new RustWorld(this.#core, handle)
  }

  /**
   * blockstate の名前を StateId に解決する。文字列が境界を越えるのはここだけ (19 §2.5)。
   * 名前の形は rse-core の StateId::parse のとおり (先頭の `minecraft:` は読み飛ばし、書いていない
   * プロパティは既定の値)。結果は覚えておき、同じ名前では wasm を呼ばない。
   * 解決できない時は、窓口の状態コード (UNKNOWN_BLOCK ほか。作業領域の誤りなら ARGUMENT) を値で返す。
   */
  tryResolve(name: string): ResolveResult {
    this.#core.live('resolve')
    const cached = this.#cache.get(name)
    if (cached !== undefined) return { ok: true, state: cached }
    const bytes = encoder.encode(name)
    // 先頭の 4 バイトを結果、その後ろを名前にする。番地は 8 バイトに揃う。
    const base = this.#core.scratch(4 + bytes.length)
    new Uint8Array(this.#core.buffer, base + 4, bytes.length).set(bytes)
    const status = this.#core.call('resolve', x => x.rse_resolve(base + 4, bytes.length, base)) >>> 0
    if (status !== 0) return { ok: false, status }
    const state = new Uint32Array(this.#core.buffer, base, 1)[0]
    this.#cache.set(name, state)
    return { ok: true, state }
  }

  /** `tryResolve` の、解決できなければ EngineError を投げる版 */
  resolve(name: string): number {
    const got = this.tryResolve(name)
    if (!got.ok) throw new EngineError(`名前を解決できない (${name})`, got.status)
    return got.state
  }
}

/**
 * ブラウザ (と fetch のある所) で、URL から読む。wasm は instantiateStreaming で読む。
 * 2 つを同時に取り、両方を待つ (片方が先に失敗しても、もう片方の失敗を取り残さない)。
 */
export async function loadEngineFromUrls(wasmUrl: string | URL, statesUrl: string | URL): Promise<Engine> {
  const states = fetch(statesUrl)
    .then(async r => {
      if (!r.ok) throw new Error(`${r.status} ${r.url}`)
      return (await r.json()) as unknown
    })
    .catch((e: unknown) => {
      throw new Error(`状態表の JSON が読めない: ${e instanceof Error ? e.message : String(e)}`, { cause: e })
    })
  const [wasm, table] = await Promise.all([fetch(wasmUrl), states])
  return Engine.load(wasm, table)
}
