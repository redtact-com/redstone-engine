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
 * - SharedArrayBuffer と Worker は使わない (19 §11 の 14)。
 */

import { ABI_VERSION, ABI_STATUS, EXPORTED_FUNCTIONS, EngineError, type RseExports } from './abi'
import { StateTable } from './states'
import { RustWorld } from './world'

/** wasm の元。バイト列・コンパイル済みの Module・fetch の Response (か、その Promise) */
export type WasmSource = BufferSource | WebAssembly.Module | Response | PromiseLike<Response>

export type ResolveResult = { readonly ok: true; readonly state: number } | { readonly ok: false; readonly status: number }

const encoder = new TextEncoder()

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

function exportsOf(instance: WebAssembly.Instance): RseExports {
  const x = instance.exports as Record<string, unknown>
  const missing = EXPORTED_FUNCTIONS.filter(name => typeof x[name] !== 'function')
  if (missing.length > 0) throw new Error(`wasm に関数が無い: ${missing.join(', ')}`)
  if (!(x.memory instanceof WebAssembly.Memory)) throw new Error('wasm が memory を書き出していない')
  return x as unknown as RseExports
}

export class Engine {
  /** 書き出し。窓口の関数は `call` を通して呼ぶ */
  readonly raw: RseExports
  readonly states: StateTable
  /**
   * 窓口を呼んだ回数。変化の窓 (ChangeWindow) が古くなったかを見るのに使う。
   * @internal
   */
  epoch = 0
  readonly #cache = new Map<string, number>()

  private constructor(raw: RseExports, states: StateTable) {
    this.raw = raw
    this.states = states
  }

  /**
   * wasm と状態表の JSON から作る。窓口の版と、状態表が wasm と同じビルドの物かを確かめる。
   * 状態表は、rse-cli が書いた JSON (文字列か、読んだ値) か、作った StateTable。
   */
  static async load(wasm: WasmSource, states: unknown): Promise<Engine> {
    const raw = exportsOf(await instantiate(wasm))
    const version = raw.rse_abi_version()
    if (version !== ABI_VERSION) throw new Error(`窓口の版が合わない: wasm は ${version}・engine-js は ${ABI_VERSION}`)
    const engine = new Engine(raw, states instanceof StateTable ? states : StateTable.parse(states))
    engine.#checkTable()
    return engine
  }

  /** 状態表の JSON が古い (wasm と別のビルド) と、名前を取り違える。各ブロックの最初と最後の状態で確かめる */
  #checkTable(): void {
    for (const b of this.states.blocks) {
      for (const id of [b.firstState, b.firstState + b.stateCount - 1]) {
        const got = this.tryResolve(this.states.name(id))
        if (!got.ok || got.state !== id) {
          throw new Error(`状態表の JSON が wasm と合わない (${this.states.name(id)})。npm run build:engine で作り直す`)
        }
      }
    }
  }

  /**
   * 窓口の関数を呼ぶ。呼ぶ前に epoch を進めるので、前に作った変化の窓は使えなくなる。
   * @internal
   */
  call<T>(f: (raw: RseExports) => T): T {
    this.epoch++
    return f(this.raw)
  }

  /**
   * 作業領域を len バイト以上に広げ、先頭の番地を返す。線形メモリが伸びうるので、窓はこの後に作る。
   * @internal
   */
  scratch(len: number): number {
    const ptr = this.call(x => x.rse_scratch(len)) >>> 0
    if (ptr === 0) throw new RangeError(`作業領域を ${len} バイトに広げられない`)
    return ptr
  }

  /** 今の線形メモリ。窓口を呼んだ後は、必ず取り直す */
  get buffer(): ArrayBuffer {
    return this.raw.memory.buffer
  }

  /** 世界を 1 つ作る。使い終えたら dispose する */
  createWorld(): RustWorld {
    const handle = this.call(x => x.rse_world_new()) >>> 0
    if (handle === 0) throw new EngineError('世界を作れない (席が尽きた)', ABI_STATUS.HANDLE)
    return new RustWorld(this, handle)
  }

  /**
   * blockstate の名前を StateId に解決する。文字列が境界を越えるのはここだけ (19 §2.5)。
   * 名前の形は rse-core の StateId::parse のとおり (先頭の `minecraft:` は読み飛ばし、書いていない
   * プロパティは既定の値)。結果は覚えておき、同じ名前では wasm を呼ばない。
   */
  tryResolve(name: string): ResolveResult {
    const cached = this.#cache.get(name)
    if (cached !== undefined) return { ok: true, state: cached }
    const bytes = encoder.encode(name)
    // 先頭の 4 バイトを結果、その後ろを名前にする。番地は 8 バイトに揃う。
    const base = this.scratch(4 + bytes.length)
    new Uint8Array(this.buffer, base + 4, bytes.length).set(bytes)
    const status = this.call(x => x.rse_resolve(base + 4, bytes.length, base)) >>> 0
    if (status !== 0) return { ok: false, status }
    const state = new Uint32Array(this.buffer, base, 1)[0]
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

/** ブラウザ (と fetch のある所) で、URL から読む。wasm は instantiateStreaming で読む */
export async function loadEngineFromUrls(wasmUrl: string | URL, statesUrl: string | URL): Promise<Engine> {
  const response = await fetch(statesUrl)
  if (!response.ok) throw new Error(`状態表の JSON を取れない: ${response.status} ${response.url}`)
  return Engine.load(fetch(wasmUrl), await response.json())
}
