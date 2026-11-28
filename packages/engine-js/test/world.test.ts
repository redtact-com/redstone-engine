import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { fileURLToPath } from 'node:url'
import {
  ABI_STATUS,
  ABI_VERSION,
  ENGINE_STATUS,
  EXPORTED_FUNCTIONS,
  EngineError,
  EngineStoppedError,
  INPUT_KIND,
  type RseExports,
  STATUS,
  STATUS_TEXT,
  statusNames,
} from '../src/abi'
import { Core, Engine, loadEngineFromUrls } from '../src/engine'
import { GENERATED_DIR, loadEngineFromFiles } from '../src/node'
import { StateTable } from '../src/states'
import { type Cell, RustWorld, releaseLeaked } from '../src/world'

/**
 * Engine・RustWorld・ChangeWindow の単体テスト (作る・捨てる・読み込む・進める・入力 (使う・置く・壊す)・時計・
 * 変化の窓・状態表の往復・誤りの断り)。
 */

const WASM = readFileSync(new URL('rse_wasm.wasm', GENERATED_DIR))
const STATES = readFileSync(new URL('states.json', GENERATED_DIR), 'utf-8')

let engine: Engine
beforeAll(async () => {
  engine = await loadEngineFromFiles()
})

const WIRE = 'redstone_wire[east=side,north=none,power=0,south=none,west=side]'

/** 石の床の上に、レバー → ダスト 3 本 → ランプ (fixture の lever-wire-lamp と同じ盤面) */
function circuit(e: Engine = engine): Cell[] {
  const at = (x: number, y: number, z: number, name: string): Cell => ({ x, y, z, state: e.resolve(name) })
  const out: Cell[] = []
  for (let x = 0; x <= 4; x++) out.push(at(x, 0, 0, 'stone'), at(x, 0, 1, 'stone'))
  out.push(at(0, 1, 0, 'lever[face=floor,facing=north,powered=false]'))
  for (let x = 1; x <= 3; x++) out.push(at(x, 1, 0, WIRE))
  out.push(at(4, 1, 0, 'redstone_lamp[lit=false]'))
  return out
}

/** 変化の窓を、名前つきのマスの列に読み出す */
function drain(world: RustWorld, e: Engine = engine): { x: number; y: number; z: number; block: string }[] {
  const w = world.changes()
  return Array.from({ length: w.count }, (_, i) => ({ x: w.x(i), y: w.y(i), z: w.z(i), block: e.states.name(w.state(i)) }))
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

/** 本物の wasm の書き出しを写し、一部の関数を差し替えた物 (trap を起こすテスト用) */
function patchedExports(patch: (real: RseExports) => Partial<RseExports>): WebAssembly.Exports {
  const real = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports as unknown as RseExports
  return { ...real, ...patch(real) } as unknown as WebAssembly.Exports
}

/** 離れずに並ぶ n マス (x は 0〜99、z は 0〜) の石 */
function manyStones(e: Engine, n: number): Int32Array {
  const stone = e.resolve('stone')
  const out = new Int32Array(n * 4)
  for (let i = 0; i < n; i++) out.set([i % 100, 0, Math.floor(i / 100), stone], i * 4)
  return out
}

describe('wasm の読み込み', () => {
  // 19 §2.5 の 7 関数 + 作業領域 + 版 + 時計の 10 関数と memory。global の 2 つ (__data_end・__heap_base) は
  // Rust の wasm32 のリンカが cdylib に出す物で、JS は使わない。顔ぶれが変わったら気づけるよう、全部を固定する。
  it('import は 0 個で、書き出しの全部が決まった顔ぶれ', () => {
    const module = new WebAssembly.Module(WASM)
    expect(WebAssembly.Module.imports(module)).toEqual([])
    const all = WebAssembly.Module.exports(module)
      .map(e => `${e.kind} ${e.name}`)
      .sort()
    const expected = [
      ...EXPORTED_FUNCTIONS.map(n => `function ${n}`),
      'memory memory',
      'global __data_end',
      'global __heap_base',
    ].sort()
    expect(all).toEqual(expected)
    expect(EXPORTED_FUNCTIONS).toHaveLength(10)
  })

  // engine-js の版と、組んだ wasm の版が同じ (字面は ABI_VERSION の 1 か所にだけ書く)。
  it('wasm の版は engine-js の ABI_VERSION と同じ', () => {
    const raw = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports as unknown as RseExports
    expect(raw.rse_abi_version()).toBe(ABI_VERSION)
    expect(engine.abiVersion).toBe(ABI_VERSION)
  })

  it('バイト列・Module・Response のどれからでも読める', async () => {
    for (const source of [WASM, new WebAssembly.Module(WASM), new Response(WASM, { headers: { 'content-type': 'application/wasm' } })]) {
      const e = await Engine.load(source, STATES)
      expect(e.abiVersion).toBe(ABI_VERSION)
      expect(e.resolve('stone')).toBe(1)
    }
    // MIME 型が違う配信 (instantiateStreaming が断る) でも、バイト列で読み直す。
    const plain = await Engine.load(Promise.resolve(new Response(WASM)), JSON.parse(STATES))
    expect(plain.states.size).toBe(engine.states.size)
  })

  // 1 つのインスタンスから Engine を 2 つ作ると epoch が分かれ、片方で線形メモリが伸びても、もう片方の窓が古いと分からない。
  it('同じインスタンスから 2 つ目の Engine は作れない', () => {
    const exports = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports
    const broken = JSON.parse(STATES)
    broken.blocks[4].defaultState = broken.blocks[4].firstState
    // 確かめで断られた時は覚えないので、JSON を直して同じインスタンスで作り直せる。
    expect(() => Engine.fromExports(exports, broken)).toThrow('状態表の JSON が wasm と合わない')
    const first = Engine.fromExports(exports, STATES)
    expect(first.resolve('stone')).toBe(1)
    expect(() => Engine.fromExports(exports, STATES)).toThrow('もう Engine がある')
    expect(() => Engine.fromExports({ ...exports }, STATES)).toThrow('もう Engine がある')
  })

  // 版の違う wasm は関数の顔ぶれも違いうるので、関数の有無より先に版を見る。
  it('版を先に確かめる', () => {
    const memory = new WebAssembly.Memory({ initial: 1 })
    const other = ABI_VERSION + 1
    expect(() => Engine.fromExports({ rse_abi_version: () => other, memory }, STATES)).toThrow(
      `窓口の版が合わない: wasm は ${other}・engine-js は ${ABI_VERSION}`,
    )
    expect(() => Engine.fromExports({ memory }, STATES)).toThrow('rse_abi_version が無い')
    expect(() => Engine.fromExports({ rse_abi_version: () => ABI_VERSION, memory }, STATES)).toThrow('wasm に関数が無い')
    // 版が合っても、時計 (版 2 で足した) が無い wasm は断る。
    const real = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports
    const withoutClock = Object.fromEntries(Object.entries(real).filter(([name]) => name !== 'rse_game_time'))
    expect(() => Engine.fromExports(withoutClock, STATES)).toThrow('wasm に関数が無い: rse_game_time')
  })

  it('状態表の JSON が wasm と合わなければ断る', async () => {
    const edited = (edit: (json: { states: { name: string[] }; blocks: { defaultState: number; firstState: number }[]; format: number }) => void) => {
      const json = JSON.parse(STATES)
      edit(json)
      return Engine.load(WASM, json)
    }
    // ブロックの中の並び替え (ダストの真ん中の 2 つを入れ替える)。最初と最後だけの確かめでは通っていた。
    await expect(
      edited(j => {
        const at = j.blocks[3].firstState + 600
        ;[j.states.name[at], j.states.name[at + 1]] = [j.states.name[at + 1], j.states.name[at]]
      }),
    ).rejects.toThrow('状態表の JSON が wasm と合わない')
    // 既定の状態の違い。
    await expect(edited(j => (j.blocks[4].defaultState = j.blocks[4].firstState))).rejects.toThrow('redstone_lamp の既定の状態')
    await expect(edited(j => (j.format = 2))).rejects.toThrow('format が 1 でない')
  })

  // ブラウザと同じく URL から読む。2 つを一緒に待つので、片方の失敗がもう片方を取り残さない
  // (取り残すと unhandled rejection になり、vitest が落ちる)。
  describe('URL から読む', () => {
    let server: Server
    let base = ''
    beforeAll(async () => {
      server = createServer((req, res) => {
        const send = (status: number, type: string, body: string | Buffer) => {
          res.writeHead(status, { 'content-type': type })
          res.end(body)
        }
        if (req.url === '/rse_wasm.wasm') send(200, 'application/wasm', WASM)
        else if (req.url === '/states.json') send(200, 'application/json', STATES)
        else if (req.url === '/broken.json') send(200, 'application/json', '{')
        // 少し待ってから、返事をせずに接続を切る (fetch そのものが reject する)。
        else if (req.url === '/cut.wasm') setTimeout(() => req.socket.destroy(), 100)
        else send(404, 'text/plain', 'no')
      })
      await new Promise<void>(done => server.listen(0, '127.0.0.1', done))
      base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
    })
    afterAll(() => new Promise<void>(done => server.close(() => done())))

    it('wasm と状態表の JSON を取って読む', async () => {
      const e = await loadEngineFromUrls(`${base}/rse_wasm.wasm`, `${base}/states.json`)
      expect(e.resolve('redstone_lamp')).toBe(engine.resolve('redstone_lamp'))
    })

    it('JSON が読めない時は、その旨で断る', async () => {
      await expect(loadEngineFromUrls(`${base}/rse_wasm.wasm`, `${base}/broken.json`)).rejects.toThrow('状態表の JSON が読めない')
      await expect(loadEngineFromUrls(`${base}/rse_wasm.wasm`, `${base}/missing.json`)).rejects.toThrow('状態表の JSON が読めない: 404')
      await expect(loadEngineFromUrls(`${base}/missing.wasm`, `${base}/states.json`)).rejects.toThrow('wasm を取れない: 404')
      // 両方とも失敗しても、1 つにまとまる。
      await expect(loadEngineFromUrls(`${base}/missing.wasm`, `${base}/broken.json`)).rejects.toThrow()
    })

    /** `f` を待つ間と、その後しばらくの間に起きた、取り残しの reject (unhandled rejection) を集める */
    async function unhandledDuring(f: () => Promise<unknown>): Promise<unknown[]> {
      const seen: unknown[] = []
      const listen = (reason: unknown) => seen.push(reason)
      process.on('unhandledRejection', listen)
      try {
        await f()
        await new Promise(done => setTimeout(done, 300))
      } finally {
        process.off('unhandledRejection', listen)
      }
      return seen
    }

    // wasm の取得そのもの (fetch) が reject する形と、壊れた状態表の JSON を組み合わせる。
    // 片方だけを待つ作りだと、先に断った後でもう片方の reject が取り残される。
    it('wasm の取得が reject しても、壊れた JSON と一緒に待って取り残さない', async () => {
      const closed = await new Promise<string>(done => {
        const s = createServer()
        s.listen(0, '127.0.0.1', () => {
          const port = (s.address() as AddressInfo).port
          s.close(() => done(`http://127.0.0.1:${port}`))
        })
      })
      // 閉じた口 (すぐ reject) と壊れた JSON。
      const fast = await unhandledDuring(() =>
        expect(loadEngineFromUrls(`${closed}/rse_wasm.wasm`, `${base}/broken.json`)).rejects.toThrow(/wasm を取れない|状態表の JSON が読めない/),
      )
      expect(fast).toEqual([])
      // 壊れた JSON が先に断られ、wasm の取得が後で reject する形。
      const late = await unhandledDuring(() =>
        expect(loadEngineFromUrls(`${base}/cut.wasm`, `${base}/broken.json`)).rejects.toThrow('状態表の JSON が読めない'),
      )
      expect(late).toEqual([])
      // wasm の取得だけが reject する時は、その旨の文で断る。
      await expect(loadEngineFromUrls(`${closed}/rse_wasm.wasm`, `${base}/states.json`)).rejects.toThrow('wasm を取れない')
    })
  })

  it('ファイルのフォルダは、末尾の / が無くても文字列でも読める', async () => {
    const dir = fileURLToPath(GENERATED_DIR)
    for (const at of [new URL(GENERATED_DIR.href.replace(/\/$/, '')), dir.replace(/\/$/, ''), dir]) {
      expect((await loadEngineFromFiles(at)).states.size).toBe(engine.states.size)
    }
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
  // 捨てた世界は wasm を呼ばずに断る。席が使い回されていても、新しい世界を動かさない。
  it('作って捨てる。捨てた世界の操作は wasm を呼ばずに HANDLE', () => {
    const a = engine.createWorld()
    const b = engine.createWorld()
    expect(a.handle).not.toBe(b.handle)
    a.dispose()
    expect(a.disposed).toBe(true)
    a.dispose() // 2 回目は何もしない
    const c = engine.createWorld() // a の席を使い回す (世代が違う)
    expect(c.handle & 0xffff).toBe(a.handle & 0xffff)
    expect(c.handle).not.toBe(a.handle)
    const before = engine.epoch
    const stone = engine.resolve('stone')
    for (const use of [
      () => a.step(),
      () => a.use(0, 1, 0),
      () => a.changes(),
      () => a.load([]),
      () => a.input('use', 'A', 0, 0, 0),
      () => a.place(0, 1, 0, stone),
      () => a.breakBlock(0, 1, 0),
      () => a.gameTime(),
      // 捨てた世界は、引数の誤りより先に断る (load ほかと同じ)。
      () => a.place(0, 1, 0, -1),
    ]) {
      expect(thrownStatus(use)).toBe(STATUS.HANDLE)
    }
    expect(engine.epoch).toBe(before)
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

  it('置けないマスがあれば投げ、1 つも置かない (高さの外も格子の枠の外も)', () => {
    const world = engine.createWorld()
    const stone = engine.resolve('stone')
    const ok = { x: 0, y: 0, z: 0, state: stone }
    expect(thrownStatus(() => world.load([ok, { ...ok, y: 320 }]))).toBe(STATUS.OUT_OF_WORLD)
    expect(thrownStatus(() => world.load([ok, { ...ok, x: 16 * 256 }]))).toBe(STATUS.OUT_OF_WORLD)
    expect(thrownStatus(() => world.load([{ ...ok, state: engine.states.size }]))).toBe(STATUS.ARGUMENT)
    expect(drain(world)).toEqual([])
    // 整数でない座標は wasm に渡す前に断る。
    expect(() => world.load([{ ...ok, x: 0.5 }])).toThrow(RangeError)
    // 組み直しは席だけ。投げずに UNIMPLEMENTED を返す。
    expect(world.load([ok], 'rebuild')).toBe(STATUS.UNIMPLEMENTED)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  // 表に無い名前 (継いだ鍵を含む) と範囲の外の数は、wasm に渡す前に断る (0 = SETTLED・use・B5 として通さない)。
  it('表に無い型・種類・区間と、範囲の外の arg を断る', () => {
    const world = engine.createWorld()
    const before = engine.epoch
    const anyName = (s: string) => s as never
    expect(() => world.load([], anyName('toString'))).toThrow('読み込みの型でない: toString')
    expect(() => world.load([], anyName('Settled'))).toThrow(RangeError)
    expect(() => world.input(anyName('nope'), 'A', 0, 0, 0)).toThrow('入力の種類でない: nope')
    expect(() => world.input(anyName('constructor'), 'A', 0, 0, 0)).toThrow(RangeError)
    expect(() => world.input('use', anyName('hasOwnProperty'), 0, 0, 0)).toThrow('入力の区間でない')
    for (const arg of [-1, 1.5, 2 ** 32, Number.NaN]) {
      expect(() => world.input('setBlock', 'C', 0, 0, 0, arg)).toThrow(RangeError)
    }
    expect(engine.epoch).toBe(before) // どれも wasm を呼んでいない
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
    expect(thrownStatus(() => world.input('setBlock', 'C', 5, 1, 0, 1 | (1 << 17)))).toBe(STATUS.ARGUMENT)
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

  it('epoch は読むだけ', () => {
    expect(() => {
      ;(engine as unknown as { epoch: number }).epoch = 0
    }).toThrow(TypeError)
  })
})

describe('置く・壊す・時計 (窓口の版 2)', () => {
  const CROSS = 'redstone_wire[east=side,north=side,power=0,south=side,west=side]'
  const LINE_X = 'redstone_wire[east=side,north=none,power=0,south=none,west=side]'

  /** 石の床 (y=0) の x 0〜4・z 0〜2 */
  function floor(e: Engine = engine): Cell[] {
    const stone = e.resolve('stone')
    const out: Cell[] = []
    for (let x = 0; x <= 4; x++) for (let z = 0; z <= 2; z++) out.push({ x, y: 0, z, state: stone })
    return out
  }

  /** 変化を座標の順に並べて読む */
  const sorted = (world: RustWorld) => drain(world).sort((p, q) => p.x - q.x || p.y - q.y || p.z - q.z)

  // 入力の種類の番号は Rust の input_kind と同じ。外の番号は wasm が ARGUMENT で断る (Rust の種類はこの 4 つだけ)。
  it('入力の種類の番号は Rust と同じ', () => {
    expect(INPUT_KIND).toEqual({ use: 0, setBlock: 1, place: 2, break: 3 })
    const raw = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports as unknown as RseExports
    const h = raw.rse_world_new()
    expect(raw.rse_input(h, 4, 2, 0, 0, 0, 0) >>> 0).toBe(STATUS.ARGUMENT)
    for (const kind of [INPUT_KIND.use, INPUT_KIND.break]) expect(raw.rse_input(h, kind, 2, 0, 0, 0, 0)).toBe(0)
  })

  // ダストの形は置いた時の規則と形状更新が決める (V06 §4.3・§4.4)。要求のプロパティは読まない。
  it('置くと周りから形が決まり、壊すと戻る', () => {
    const world = engine.createWorld()
    world.load(floor())
    drain(world)
    expect(world.place(1, 1, 1, engine.resolve('redstone_wire'))).toBe(0)
    expect(drain(world)).toEqual([{ x: 1, y: 1, z: 1, block: CROSS }])
    expect(world.place(2, 1, 1, engine.resolve(CROSS))).toBe(0)
    expect(sorted(world)).toEqual([
      { x: 1, y: 1, z: 1, block: LINE_X },
      { x: 2, y: 1, z: 1, block: LINE_X },
    ])
    expect(world.breakBlock(2, 1, 1)).toBe(0)
    expect(sorted(world)).toEqual([
      { x: 1, y: 1, z: 1, block: CROSS },
      { x: 2, y: 1, z: 1, block: 'air' },
    ])
    // レバーは face と facing を要求のまま使う。区間を渡せる (input と同じ)。
    expect(world.place(3, 1, 1, engine.resolve('lever[face=floor,facing=east]'), 'C')).toBe(0)
    expect(world.input('place', 'A', 4, 1, 1, engine.resolve('redstone_lamp'))).toBe(0)
    expect(sorted(world)).toEqual([
      { x: 3, y: 1, z: 1, block: 'lever[face=floor,facing=east,powered=false]' },
      { x: 4, y: 1, z: 1, block: 'redstone_lamp[lit=false]' },
    ])
    expect(world.input('break', 'B5', 3, 1, 1)).toBe(0)
    expect(drain(world)).toEqual([{ x: 3, y: 1, z: 1, block: 'air' }])
    world.dispose()
  })

  // 実機でも何も起きない所は 0 を返す。規則の無い要求は投げずに UNIMPLEMENTED を返す (エンジンの状態コード)。
  it('置けない所と空気を壊すのは 0、規則の無い要求は UNIMPLEMENTED', () => {
    const world = engine.createWorld()
    world.load(floor())
    drain(world)
    expect(world.place(0, 0, 0, engine.resolve('redstone_lamp'))).toBe(0) // 石の上書き
    expect(world.place(1, 3, 1, engine.resolve('redstone_wire'))).toBe(0) // 支えが無い
    expect(world.breakBlock(1, 1, 1)).toBe(0) // 空気
    expect(world.place(1, 1, 1, engine.resolve('lever[face=floor,powered=true]'))).toBe(STATUS.UNIMPLEMENTED)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  // 窓口は置く前に高さと格子の枠を確かめ、OUT_OF_WORLD を返す (SAFETY に落とさない)。壊すは範囲を確かめない。
  it('高さの外・格子の枠の外に置くと OUT_OF_WORLD を投げ、壊すは 0', () => {
    const world = engine.createWorld()
    const stone = engine.resolve('stone')
    world.load([{ x: 0, y: 0, z: 0, state: stone }])
    drain(world)
    for (const [x, y, z] of [
      [0, 320, 0],
      [0, -65, 0],
      [16 * 256, 1, 0],
      [0, 1, -16 * 256],
    ]) {
      expect(thrownStatus(() => world.place(x, y, z, stone))).toBe(STATUS.OUT_OF_WORLD)
      expect(world.breakBlock(x, y, z)).toBe(0)
    }
    expect(drain(world)).toEqual([])
    expect(world.step()).toBe(0) // 後から SAFETY が出てこない
    expect(world.place(16 * 255, 0, 0, stone)).toBe(0)
    expect(drain(world)).toEqual([{ x: 16 * 255, y: 0, z: 0, block: 'stone' }])
    world.dispose()
  })

  // StateId の形でない state は wasm に渡す前に断る。表の外・空気・上のビットは wasm が ARGUMENT で断る。
  it('置く state と arg の断り', () => {
    const world = engine.createWorld()
    world.load(floor())
    drain(world)
    const before = engine.epoch
    for (const state of [-1, 1.5, 0x10000, Number.NaN, Number.POSITIVE_INFINITY, '1' as unknown as number]) {
      expect(() => world.place(1, 1, 1, state)).toThrow(RangeError)
    }
    expect(() => world.place(1.5, 1, 1, 1)).toThrow(RangeError)
    expect(() => world.breakBlock(1, 2 ** 31, 1)).toThrow(RangeError)
    expect(engine.epoch).toBe(before) // どれも wasm を呼んでいない
    const stone = engine.resolve('stone')
    expect(thrownStatus(() => world.place(1, 1, 1, engine.states.size))).toBe(STATUS.ARGUMENT)
    expect(thrownStatus(() => world.place(1, 1, 1, 0))).toBe(STATUS.ARGUMENT) // 空気
    expect(thrownStatus(() => world.input('place', 'A', 1, 1, 1, stone | (1 << 16)))).toBe(STATUS.ARGUMENT)
    expect(thrownStatus(() => world.input('break', 'A', 0, 0, 0, 1))).toBe(STATUS.ARGUMENT)
    expect(drain(world)).toEqual([])
    world.dispose()
  })

  // 時計は L5 のゲーム時刻 (V01 §2.3)。読み込みと入力では進まず、step(n) で n 進む。世界ごとに別。
  it('時計は step で進み、読み込みと入力では進まない', () => {
    const [a, b] = [engine.createWorld(), engine.createWorld()]
    expect(a.gameTime()).toBe(0)
    a.load(circuit())
    a.use(0, 1, 0)
    a.place(0, 1, 1, engine.resolve('redstone_wire'))
    a.breakBlock(0, 1, 1)
    expect(a.gameTime()).toBe(0)
    a.step(3)
    expect(a.gameTime()).toBe(3)
    a.step(0)
    expect(a.gameTime()).toBe(3)
    a.step(5)
    expect([a.gameTime(), b.gameTime()]).toEqual([8, 0])
    b.step()
    expect([a.gameTime(), b.gameTime()]).toEqual([8, 1])
    a.dispose()
    expect(thrownStatus(() => a.gameTime())).toBe(STATUS.HANDLE)
    expect(b.gameTime()).toBe(1)
    b.dispose()
  })

  // 時計を読むのも窓口の呼び出しなので、epoch が進み、前の変化の窓は古くなる。線形メモリは伸ばさない。
  it('時計を読むと変化の窓が古くなる', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const w = world.changes()
    expect(w.valid).toBe(true)
    const [epoch, buffer] = [engine.epoch, engine.memory.buffer]
    expect(world.gameTime()).toBe(0)
    expect(engine.epoch).toBe(epoch + 1)
    expect(engine.memory.buffer).toBe(buffer)
    expect(w.valid).toBe(false)
    expect(() => w.state(0)).toThrow('変化の窓は')
    world.dispose()
  })

  // i64 は BigInt で来る。-1n はハンドルが無い印、0 以上の安全な整数の外は number に直せない。
  it('時計の BigInt を number に直す', () => {
    let value = 0n
    const e = Engine.fromExports(patchedExports(() => ({ rse_game_time: () => value })), STATES)
    const world = e.createWorld()
    for (const ok of [0n, 1n, BigInt(Number.MAX_SAFE_INTEGER)]) {
      value = ok
      expect(world.gameTime()).toBe(Number(ok))
    }
    value = -1n
    expect(thrownStatus(() => world.gameTime())).toBe(STATUS.HANDLE)
    for (const bad of [BigInt(Number.MAX_SAFE_INTEGER) + 1n, 2n ** 63n - 1n, -2n, -(2n ** 63n)]) {
      value = bad
      expect(() => world.gameTime()).toThrow(RangeError)
    }
    world.dispose()
  })
})

describe('変化の窓と線形メモリ', () => {
  // 変化は線形メモリの窓で読み、写しを作らない (19 §2.5)。窓は次に窓口を呼ぶと使えない。
  it('窓は線形メモリそのもので、次の呼び出しで古くなる', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const w = world.changes()
    expect(w.count).toBe(15)
    expect(w.world).toBe(world)
    expect(w.data.buffer).toBe(engine.memory.buffer)
    expect(w.data.length).toBe(15 * 4)
    expect([w.x(14), w.y(14), w.z(14)]).toEqual([4, 1, 0])
    for (const i of [15, -1, 0.5]) expect(() => w.state(i)).toThrow(RangeError)
    expect(w.valid).toBe(true)
    world.step()
    expect(w.valid).toBe(false)
    expect(() => w.state(0)).toThrow('変化の窓は')
    world.dispose()
  })

  // 窓は epoch に頼らず、配列の buffer が今の memory.buffer かも見る。外から memory.grow を呼んで切り離されても、読まずに断る。
  it('窓口を通さずに線形メモリが伸びても、古い窓は読まずに断る', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const w = world.changes()
    const epoch = engine.epoch
    engine.memory.grow(1)
    expect(engine.epoch).toBe(epoch)
    expect(w.valid).toBe(false)
    expect(() => w.x(0)).toThrow('変化の窓は')
    expect(() => w.data).toThrow('変化の窓は')
    world.dispose()
  })

  // 呼び出しと呼び出しの間で伸びる形: 前の ArrayBuffer は切り離される。呼んだ後に作り直した窓で読める。
  it('呼び出しの間に線形メモリが伸びても、作り直した窓で読める', () => {
    const world = engine.createWorld()
    world.load(circuit())
    const before = engine.memory.buffer
    const stale = new Int32Array(before)
    engine.tryResolve('x'.repeat(before.byteLength + (1 << 20)))
    expect(engine.memory.buffer).not.toBe(before)
    expect(stale.length).toBe(0)
    const w = world.changes()
    expect(w.count).toBe(15)
    expect(engine.states.name(w.state(14))).toBe('redstone_lamp[lit=false]')
    world.dispose()
  })

  // 呼び出しの中で伸びる形: rse_changes が列を伸ばす時と、load が作業領域を広げる時。
  // 新しいインスタンスで、作業領域を小さいまま (1,000 マスずつ) 10 万マスを置いてから確かめる。
  it('rse_changes と load の中で線形メモリが伸びても読める・変化の窓の data を load に渡せる', async () => {
    const e = await Engine.load(WASM, STATES)
    const [a, b] = [e.createWorld(), e.createWorld()]
    const cells = manyStones(e, 100_000)
    for (let at = 0; at < cells.length; at += 4_000) a.load(cells.subarray(at, at + 4_000))
    const beforeChanges = e.memory.buffer
    const w = a.changes()
    expect(e.memory.buffer).not.toBe(beforeChanges) // rse_changes の中で伸びた
    expect(w.count).toBe(100_000)
    expect([w.x(99_999), w.z(99_999), e.states.name(w.state(99_999))]).toEqual([99, 999, 'stone'])
    // 変化の窓の data (線形メモリの上の配列) を、そのまま別の世界に置く。作業領域を広げる時に伸びても、写してから使う。
    const beforeLoad = e.memory.buffer
    expect(b.load(w.data)).toBe(0)
    expect(e.memory.buffer).not.toBe(beforeLoad) // load の中で伸びた
    const placed = b.changes()
    expect(placed.count).toBe(100_000)
    expect(Array.from(placed.data.subarray(0, 8))).toEqual(Array.from(cells.subarray(0, 8)))
    a.dispose()
    b.dispose()
  })
})

describe('安全網と trap', () => {
  // 安全網は Core を通して捨てる。epoch が進むので、ほかの世界の窓も古くなる (rse_world_free も線形メモリを伸ばしうる)。
  it('安全網は epoch を進め、例外を外へ出さない', () => {
    const real = new WebAssembly.Instance(new WebAssembly.Module(WASM), {}).exports as unknown as RseExports
    const core = new Core(real)
    const [a, b] = [1, 2].map(() => new RustWorld(core, core.call('new', x => x.rse_world_new())))
    a.load([])
    const w = a.changes()
    releaseLeaked({ core, handle: b.handle })
    expect(w.valid).toBe(false)
    expect(core.call('free', x => x.rse_world_free(b.handle))).toBe(STATUS.HANDLE) // もう捨てた
    // 2 回目 (もう無いハンドル) も、例外を出さない。
    expect(() => releaseLeaked({ core, handle: b.handle })).not.toThrow()
    a.dispose()
  })

  // release は panic = abort なので、wasm の中の panic は trap になり、その後は表の借用が戻らない。
  // ここでは rse_step を trap させる書き出しで、engine-js の扱いを確かめる。
  it('trap の後は Engine が止まり、分かる文で投げる', () => {
    let trap = false
    let calls = 0
    const e = Engine.fromExports(
      patchedExports(real => ({
        rse_step: (h: number, n: number) => {
          if (trap) throw new WebAssembly.RuntimeError('unreachable')
          return real.rse_step(h, n)
        },
        rse_world_new: () => {
          calls++
          return real.rse_world_new()
        },
      })),
      STATES,
    )
    const world = e.createWorld()
    world.load(circuit(e))
    expect(world.step()).toBe(0)
    trap = true
    expect(() => world.step()).toThrow(EngineStoppedError)
    expect(e.stopped).toBe(true)
    // 以後は wasm を呼ばずに、分かる文で投げる。
    const before = calls
    expect(() => e.createWorld()).toThrow('Engine ごと捨てて読み直す')
    expect(calls).toBe(before)
    expect(() => e.resolve('redstone_lamp[lit=true]')).toThrow(EngineStoppedError)
    // 文の頭は呼んだ操作の名前 (内部の scratch ではない)。
    expect(() => world.changes()).toThrow(/^changes: .*Engine ごと捨てて読み直す/)
    expect(() => world.load([])).toThrow(/^load: /)
    expect(() => world.step()).toThrow(/^step: /)
    expect(() => e.resolve('lever')).toThrow(/^resolve: /)
    // dispose は投げない (finally の中で、元の例外を隠さない)。安全網も呼ばない。
    expect(() => world.dispose()).not.toThrow()
    const core = new Core(patchedExports(() => ({})) as unknown as RseExports)
    const handle = core.call('new', x => x.rse_world_new())
    try {
      core.call('trap', () => {
        throw new WebAssembly.RuntimeError('unreachable')
      })
    } catch {
      // 止める
    }
    const epoch = core.epoch
    releaseLeaked({ core, handle })
    expect(core.epoch).toBe(epoch)
  })

  // 安全網の中で trap が起きても、回収の時に投げない (受ける人がいない)。Engine は止まった状態になる。
  it('安全網の中の trap は投げず、Engine を止める', () => {
    const core = new Core(
      patchedExports(() => ({
        rse_world_free: () => {
          throw new WebAssembly.RuntimeError('unreachable')
        },
      })) as unknown as RseExports,
    )
    const handle = core.call('new', x => x.rse_world_new())
    expect(() => releaseLeaked({ core, handle })).not.toThrow()
    expect(core.stopped).toBe(true)
    expect(() => core.call('step', x => x.rse_step(handle, 1))).toThrow(EngineStoppedError)
  })
})
