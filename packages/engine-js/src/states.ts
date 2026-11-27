/**
 * 状態表: StateId と blockstate の名前・性質のビットを行き来する。
 *
 * 中身は rse-cli が書き出す JSON (`rse states`。crates/rse-cli/src/states.rs) で、生成物なので追跡しない。
 * StateId はエンジンの内部だけの番号で、実機の状態 ID ではない (19 §3.1)。
 *
 * - `name(id)` は、プロパティを名前順で全部書いた形 (`redstone_lamp[lit=false]`) を返す。fixture と同じ書き方。
 * - `id(name)` は、その形の名前だけを引く。`minecraft:` 付き・プロパティを省いた形などは、
 *   `Engine.resolve` (wasm の rse_resolve) で解決する。
 */

/** この形の版 (rse-cli の states::FORMAT) */
export const STATE_TABLE_FORMAT = 1

export interface PropInfo {
  readonly name: string
  /** 値は実機の並び */
  readonly values: readonly string[]
}

export interface BlockInfo {
  readonly name: string
  /** 振る舞いの種類 (rse-core の Behavior の名前) */
  readonly behavior: string
  readonly firstState: number
  readonly stateCount: number
  readonly defaultState: number
  /** プロパティ (名前順) */
  readonly props: readonly PropInfo[]
}

export interface BitName {
  readonly name: string
  readonly bit: number
}

/** rse-cli が書き出す JSON の形 */
export interface StateTableJson {
  readonly format: number
  readonly stateCount: number
  readonly flagBits: readonly BitName[]
  readonly statusBits: readonly BitName[]
  readonly blocks: readonly BlockInfo[]
  readonly states: { readonly name: readonly string[]; readonly flags: readonly number[] }
}

function fail(what: string): never {
  throw new Error(`状態表の JSON が読めない: ${what}`)
}

export class StateTable {
  /** 状態の総数。StateId は 0 からこの数の手前まで */
  readonly size: number
  readonly blocks: readonly BlockInfo[]
  /** 性質のビットの名前 → ビット */
  readonly flagBits: ReadonlyMap<string, number>
  /** エンジンの状態コードの名前 → ビット (rse-core の Status::ALL) */
  readonly statusBits: ReadonlyMap<string, number>
  readonly #names: readonly string[]
  readonly #flags: Uint16Array
  readonly #blockOf: Uint16Array
  readonly #byName: Map<string, number>

  private constructor(json: StateTableJson) {
    this.size = json.stateCount
    this.blocks = json.blocks
    this.flagBits = new Map(json.flagBits.map(b => [b.name, b.bit]))
    this.statusBits = new Map(json.statusBits.map(b => [b.name, b.bit]))
    this.#names = json.states.name
    this.#flags = Uint16Array.from(json.states.flags)
    this.#blockOf = new Uint16Array(this.size)
    json.blocks.forEach((b, i) => this.#blockOf.fill(i, b.firstState, b.firstState + b.stateCount))
    this.#byName = new Map(this.#names.map((n, i) => [n, i]))
  }

  /** JSON (文字列か、読んだ値) から作る。形が合わなければ例外 */
  static parse(json: unknown): StateTable {
    const value = (typeof json === 'string' ? JSON.parse(json) : json) as Partial<StateTableJson> | null
    if (value === null || typeof value !== 'object') fail('オブジェクトでない')
    if (value.format !== STATE_TABLE_FORMAT) fail(`format が ${STATE_TABLE_FORMAT} でない (${String(value.format)})`)
    const n = value.stateCount
    if (typeof n !== 'number' || !Number.isInteger(n) || n < 1 || n > 0x10000) fail('stateCount')
    if (!Array.isArray(value.states?.name) || value.states.name.length !== n) fail('states.name の数')
    if (!Array.isArray(value.states.flags) || value.states.flags.length !== n) fail('states.flags の数')
    if (!Array.isArray(value.flagBits) || !Array.isArray(value.statusBits)) fail('flagBits・statusBits')
    if (!Array.isArray(value.blocks) || value.blocks.length === 0) fail('blocks')
    // ブロックの状態は、最初の StateId から続けて並ぶ (StateId → ブロックを引くのに使う)。
    let next = 0
    for (const b of value.blocks) {
      if (b.firstState !== next || !(b.stateCount >= 1)) fail(`blocks の ${b.name} の範囲`)
      next += b.stateCount
    }
    if (next !== n) fail('blocks の状態の合計')
    return new StateTable(value as StateTableJson)
  }

  #check(id: number): void {
    if (!Number.isInteger(id) || id < 0 || id >= this.size) throw new RangeError(`StateId ${id} は表の外`)
  }

  /** blockstate の名前 (プロパティは名前順で全部) */
  name(id: number): string {
    this.#check(id)
    return this.#names[id]
  }

  /** `name(id)` の形の名前から引く。無ければ undefined */
  id(name: string): number | undefined {
    return this.#byName.get(name)
  }

  /** 性質のビット */
  flags(id: number): number {
    this.#check(id)
    return this.#flags[id]
  }

  /** 名前の付いた性質を持つか (例: `has(id, 'CONDUCTOR')`)。知らない名前は例外 */
  has(id: number, flag: string): boolean {
    const bit = this.flagBits.get(flag)
    if (bit === undefined) throw new RangeError(`知らない性質: ${flag}`)
    return (this.flags(id) & bit) !== 0
  }

  /** その状態のブロック */
  block(id: number): BlockInfo {
    this.#check(id)
    return this.blocks[this.#blockOf[id]]
  }
}
