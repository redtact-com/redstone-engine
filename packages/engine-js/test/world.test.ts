import { beforeAll, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import {
  ABI_STATUS,
  ENGINE_STATUS,
  EXPORTED_FUNCTIONS,
  EngineError,
  STATUS,
  STATUS_TEXT,
  statusNames,
} from '../src/abi'
import { Engine } from '../src/engine'
import { GENERATED_DIR, loadEngineFromFiles } from '../src/node'
import { StateTable } from '../src/states'
import type { Cell, RustWorld } from '../src/world'

/** RustWorld と Engine の単体テスト (作る・捨てる・置く・進める・入力・変化の窓・状態表の往復)。 */

const WASM = readFileSync(new URL('rse_wasm.wasm', GENERATED_DIR))
const STATES = readFileSync(new URL('states.json', GENERATED_DIR), 'utf-8')

let engine: Engine
beforeAll(async () => {
  engine = await loadEngineFromFiles()
})

const WIRE = 'redstone_wire[east=side,north=none,power=0,south=none,west=side]'

/** 石の床の上に、レバー → ダスト 3 本 → ランプ (fixture の lever-wire-lamp と同じ盤面) */
function circuit(): Cell[] {
  const at = (x: number, y: number, z: number, name: string): Cell => ({ x, y, z, state: engine.resolve(name) })
  const out: Cell[] = []
  for (let x = 0; x <= 4; x++) out.push(at(x, 0, 0, 'stone'), at(x, 0, 1, 'stone'))
  out.push(at(0, 1, 0, 'lever[face=floor,facing=north,powered=false]'))
  for (let x = 1; x <= 3; x++) out.push(at(x, 1, 0, WIRE))
  out.push(at(4, 1, 0, 'redstone_lamp[lit=false]'))
  return out
}

/** 変化の窓を、名前つきのマスの列に読み出す */
function drain(world: RustWorld): { x: number; y: number; z: number; block: string }[] {
  const w = world.changes()
  return Array.from({ length: w.count }, (_, i) => ({ x: w.x(i), y: w.y(i), z: w.z(i), block: engine.states.name(w.state(i)) }))
}

function thrownStatus(f: () => unknown): number {
  try {
    f()
  } catch (e) {
    if (e instanceof EngineError) return e.status
    throw e
  }
  throw new Error('投げなかった')
}

describe('wasm の読み込み', () => {
  it('import は 0 個で、書き出しは 9 関数と memory', () => {
    const module = new WebAssembly.Module(WASM)
    expect(WebAssembly.Module.imports(module)).toEqual([])
    const functions = WebAssembly.Module.exports(module).filter(e => e.kind === 'function').map(e => e.name)
    expect(functions.sort()).toEqual([...EXPORTED_FUNCTIONS].sort())
    expect(WebAssembly.Module.exports(module)).toContainEqual({ name: 'memory', kind: 'memory' })
  })

  it('バイト列・Module・Response のどれからでも読める', async () => {
    for (const source of [WASM, new WebAssembly.Module(WASM), new Response(WASM, { headers: { 'content-type': 'application/wasm' } })]) {
      const e = await Engine.load(source, STATES)
      expect(e.raw.rse_abi_version()).toBe(1)
      expect(e.resolve('stone')).toBe(1)
    }
    // MIME 型が違う配信 (instantiateStreaming が断る) でも、バイト列で読み直す。
    const plain = await Engine.load(Promise.resolve(new Response(WASM)), JSON.parse(STATES))
    expect(plain.states.size).toBe(engine.states.size)
  })

  it('状態表の JSON が wasm と合わなければ断る', async () => {
    const json = JSON.parse(STATES)
    // 名前を 2 つ入れ替えた表 (古い JSON を読んだ時に起きる取り違え)。
    const names = json.states.name
    const last = json.blocks[4].firstState + 1
    ;[names[last - 1], names[last]] = [names[last], names[last - 1]]
    await expect(Engine.load(WASM, json)).rejects.toThrow('状態表の JSON が wasm と合わない')
    await expect(Engine.load(WASM, { ...JSON.parse(STATES), format: 2 })).rejects.toThrow('format が 1 でない')
  })
})

describe('状態表', () => {
  it('全部の StateId が、名前を通して往復する (表でも wasm でも)', () => {
    const t = engine.states
    expect(t.size).toBe(1 + 1 + 24 + 1296 + 2)
    for (let id = 0; id < t.size; id++) {
      const name = t.name(id)
      expect(t.id(name)).toBe(id)
      expect(engine.resolve(name)).toBe(id)
    }
    expect(() => t.name(t.size)).toThrow(RangeError)
    expect(() => t.name(-1)).toThrow(RangeError)
  })

  it('性質のビットとブロックを引ける', () => {
    const t = engine.states
    const stone = engine.resolve('stone')
    const lever = engine.resolve('lever')
    expect(t.has(stone, 'CONDUCTOR')).toBe(true)
    expect(t.has(lever, 'CONDUCTOR')).toBe(false)
    expect(t.has(lever, 'SIGNAL_SOURCE')).toBe(true)
    expect(t.has(0, 'AIR')).toBe(true)
    expect(() => t.has(0, 'NOPE')).toThrow(RangeError)
    const block = t.block(engine.resolve('redstone_lamp[lit=true]'))
    expect(block.name).toBe('redstone_lamp')
    expect(block.behavior).toBe('Lamp')
    expect(t.name(block.defaultState)).toBe('redstone_lamp[lit=false]')
    expect(block.props).toEqual([{ name: 'lit', values: ['true', 'false'] }])
  })

  // エンジンの状態コードの表は、Rust の Status::ALL (状態表の JSON に書き出した物) と同じ。
  it('状態コードの表は Rust と同じ', () => {
    expect(Object.fromEntries(engine.states.statusBits)).toEqual(ENGINE_STATUS)
    for (const bit of Object.values(ABI_STATUS)) expect(bit & 0xffff).toBe(0)
    expect(Object.keys(STATUS_TEXT).sort()).toEqual(Object.keys(STATUS).sort())
    expect(statusNames(STATUS.UNIMPLEMENTED | STATUS.HANDLE | (1 << 30))).toEqual(['UNIMPLEMENTED', 'HANDLE', 'bit 30'])
    expect(StateTable.parse(STATES).size).toBe(engine.states.size)
  })
})

describe('名前の解決', () => {
  it('rse-core の StateId::parse のとおりに解決する', () => {
    expect(engine.resolve('minecraft:redstone_lamp')).toBe(engine.resolve('redstone_lamp[lit=false]'))
    expect(engine.resolve('lever[powered=true]')).toBe(engine.resolve('lever[face=wall,facing=north,powered=true]'))
  })

  it('解決できない理由を状態コードで返す', () => {
    const why = (name: string) => {
      const got = engine.tryResolve(name)
      return got.ok ? 'ok' : statusNames(got.status)
    }
    expect(why('repeater')).toEqual(['UNKNOWN_BLOCK'])
    expect(why('redstone_lamp[on=true]')).toEqual(['UNKNOWN_PROPERTY'])
    expect(why('redstone_lamp[lit=yes]')).toEqual(['UNKNOWN_VALUE'])
    expect(why('redstone_lamp[lit=true')).toEqual(['SYNTAX'])
    expect(thrownStatus(() => engine.resolve('repeater'))).toBe(STATUS.UNKNOWN_BLOCK)
    // 長い名前でも作業領域を広げて渡す。
    expect(why(`stone${'x'.repeat(100_000)}`)).toEqual(['UNKNOWN_BLOCK'])
  })
})

describe('RustWorld', () => {
  it('作って捨てる。捨てた世界を使うと HANDLE', () => {
    const a = engine.createWorld()
    const b = engine.createWorld()
    expect(a.handle).not.toBe(b.handle)
    a.dispose()
    expect(a.disposed).toBe(true)
    a.dispose() // 2 回目は何もしない
    for (const use of [() => a.step(), () => a.use(0, 1, 0), () => a.changes(), () => a.load([])]) {
      expect(thrownStatus(use)).toBe(STATUS.HANDLE)
    }
    // 捨てた席を使い回しても、古いハンドルは新しい世界に当たらない。
    const c = engine.createWorld()
    expect(c.handle).not.toBe(a.handle)
    expect(thrownStatus(() => a.step())).toBe(STATUS.HANDLE)
    b.dispose()
    c.dispose()
  })

  it('置いたマスは変化の列に載り、更新を出さない', () => {
    const world = engine.createWorld()
    expect(world.load(circuit())).toBe(0)
    const placed = drain(world)
    expect(placed).toHaveLength(15)
    expect(placed[14]).toEqual({ x: 4, y: 1, z: 0, block: 'redstone_lamp[lit=false]' })
    expect(drain(world)).toEqual([])
    expect(world.step(8)).toBe(0)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  it('置けないマスがあれば投げ、1 つも置かない', () => {
    const world = engine.createWorld()
    const stone = engine.resolve('stone')
    expect(thrownStatus(() => world.load([{ x: 0, y: 0, z: 0, state: stone }, { x: 0, y: 320, z: 0, state: stone }]))).toBe(
      STATUS.OUT_OF_WORLD,
    )
    expect(thrownStatus(() => world.load([{ x: 0, y: 0, z: 0, state: engine.states.size }]))).toBe(STATUS.ARGUMENT)
    expect(drain(world)).toEqual([])
    // 整数でない座標は wasm に渡す前に断る。
    expect(() => world.load([{ x: 0.5, y: 0, z: 0, state: stone }])).toThrow(RangeError)
    // 組み直しは席だけ。投げずに UNIMPLEMENTED を返す。
    expect(world.load([{ x: 0, y: 0, z: 0, state: stone }], 'rebuild')).toBe(STATUS.UNIMPLEMENTED)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  // レバーを入れるとその場でランプが点き、戻すと 4gt 後に消える (V11 §4.1)。
  it('進める・入力・変化の窓', () => {
    const world = engine.createWorld()
    world.load(circuit())
    drain(world)
    expect(world.use(0, 1, 0)).toBe(0)
    expect(drain(world).map(c => c.block)).toEqual([
      'lever[face=floor,facing=north,powered=true]',
      'redstone_wire[east=side,north=none,power=15,south=none,west=side]',
      'redstone_wire[east=side,north=none,power=14,south=none,west=side]',
      'redstone_wire[east=side,north=none,power=13,south=none,west=side]',
      'redstone_lamp[lit=true]',
    ])
    world.use(0, 1, 0)
    expect(drain(world).some(c => c.x === 4)).toBe(false)
    world.step(3)
    expect(drain(world)).toEqual([])
    world.step()
    expect(drain(world)).toEqual([{ x: 4, y: 1, z: 0, block: 'redstone_lamp[lit=false]' }])
    world.dispose()
  })

  it('入力の誤りは投げ、席だけの入力は UNIMPLEMENTED を返す', () => {
    const world = engine.createWorld()
    world.load(circuit())
    drain(world)
    expect(thrownStatus(() => world.input('use', 'A', 0, 1, 0, 1))).toBe(STATUS.ARGUMENT)
    expect(world.input('setBlock', 'C', 5, 1, 0, engine.resolve('stone'))).toBe(STATUS.UNIMPLEMENTED)
    expect(thrownStatus(() => world.input('setBlock', 'C', 5, 1, 0, engine.states.size))).toBe(STATUS.ARGUMENT)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  it('世界どうしは別々に動く', () => {
    const [a, b] = [engine.createWorld(), engine.createWorld()]
    a.load(circuit())
    b.load(circuit())
    drain(a)
    drain(b)
    a.use(0, 1, 0)
    expect(drain(a)).toHaveLength(5)
    expect(drain(b)).toEqual([])
    a.dispose()
    b.use(0, 1, 0)
    expect(drain(b).at(-1)).toEqual({ x: 4, y: 1, z: 0, block: 'redstone_lamp[lit=true]' })
    b.dispose()
  })

  // 変化は線形メモリの窓で読み、写しを作らない (19 §2.5)。窓は次に窓口を呼ぶと使えない。
  it('変化の窓は線形メモリそのもので、次の呼び出しで古くなる', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const w = world.changes()
    expect(w.count).toBe(15)
    expect(w.data.buffer).toBe(engine.raw.memory.buffer)
    expect(w.data.length).toBe(15 * 4)
    expect([w.x(14), w.y(14), w.z(14)]).toEqual([4, 1, 0])
    expect(w.valid).toBe(true)
    world.step()
    expect(w.valid).toBe(false)
    expect(() => w.state(0)).toThrow('変化の窓は')
    world.dispose()
  })

  // 線形メモリが伸びると、前の ArrayBuffer は切り離される。窓口を呼んだ後に窓を作り直せば読める。
  it('線形メモリが伸びた後も、作り直した窓で読める', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const before = engine.raw.memory.buffer
    const stale = new Int32Array(before)
    engine.scratch(before.byteLength + (1 << 20))
    expect(engine.raw.memory.buffer).not.toBe(before)
    expect(stale.length).toBe(0)
    const w = world.changes()
    expect(w.count).toBe(15)
    expect(engine.states.name(w.state(14))).toBe('redstone_lamp[lit=false]')
    world.dispose()
  })
})
