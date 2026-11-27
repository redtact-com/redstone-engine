/**
 * fixture の実行器 (wasm 版)。native の実行器 (crates/rse-cli/src/fixture.rs) と同じ数え方・同じ断り方にする。
 *
 * - 初めの盤面は tick −1 の状態。t = 0 では step を呼ばず、tick 0 の入力だけを入れて読む。
 *   step を呼ぶのは t ≧ 1 だけ (19 §4.1・conformance/fixtures/SOURCES.md)。
 * - 盤面は更新を出さずに置く (load の settled)。
 * - 期待は変化のあった tick の差分。差分を積んだ盤面と、領域の全マスを毎 tick 比べる。書いていないマスは空気。
 * - 実際の盤面は、変化の窓 (rse_changes) だけで直した写しである。wasm から 1 マスずつ読む関数は無いので、
 *   変化の列の抜け (書き出さなかった変化) と違う状態は、ここで不一致として出る。余り (変わっていないマスの
 *   書き出し) は写しを変えないので出ない。余りが無いこと (正味の変化) は rse-wasm と rse-core の単体テストが見る。
 * - 状態コードが立ったら不一致と同じに扱う。
 * - 表に無いブロック・知らない入力・読まない欄は、エラーで止める。文は native と同じにする。
 */

import type { Engine } from '../src/engine'
import { ABI_STATUS, EngineError, SECTION, type Section, statusNames } from '../src/abi'

export interface Pos {
  readonly x: number
  readonly y: number
  readonly z: number
}

export interface Fixture {
  name: string
  mcVersion: string
  ticks: number
  from: Pos
  to: Pos
  blocks: { pos: Pos; state: number }[]
  /** fixture に書いてある並びのまま */
  inputs: { tick: number; section: Section; pos: Pos }[]
  expect: { tick: number; changes: { pos: Pos; block: string }[] }[]
}

export interface Mismatch {
  tick: number
  pos: Pos
  expect: string
  actual: string
}

export interface Report {
  name: string
  ticks: number
  /** step を呼んだ回数。t = 0 では呼ばないので、ticks と同じになる */
  stepped: number
  /** 比べたマスの数 (tick ごとの領域の全マスの合計) */
  compared: number
  mismatches: Mismatch[]
  /** 状態コードが立った tick と、その値 */
  statuses: [number, number][]
}

export function passed(r: Report): boolean {
  return r.mismatches.length === 0 && r.statuses.length === 0
}

/** native の Report の Display と同じ形で書く */
export function formatReport(r: Report): string {
  if (passed(r)) return `${r.name}: 一致 (tick 0〜${r.ticks}・${r.compared} マス)`
  const lines = [`${r.name}: 不一致 ${r.mismatches.length} マス・状態コード ${r.statuses.length} 件`]
  for (const m of r.mismatches) {
    lines.push(`  tick ${m.tick} (${m.pos.x}, ${m.pos.y}, ${m.pos.z}): 期待 ${m.expect} / 実際 ${m.actual}`)
  }
  for (const [tick, status] of r.statuses) lines.push(`  tick ${tick}: 状態コード ${statusNames(status).join(' | ')}`)
  return lines.join('\n')
}

const ROOT_KEYS = ['name', 'description', 'mcVersion', 'ticks', 'region', 'blocks', 'inputs', 'expect', 'generated']
const REGION_KEYS = ['from', 'to']
const BLOCK_KEYS = ['pos', 'block']
const USE_INPUT_KEYS = ['tick', 'pos', 'action']
const EXPECT_KEYS = ['tick', 'changes']
const CHANGE_KEYS = ['pos', 'block']

type Json = Record<string, unknown>

function onlyKnown(value: unknown, place: string, known: string[]): Json {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${place} がオブジェクトでない`)
  // native (serde_json の Map) と同じく、名前順に並べて出す。
  const extra = Object.keys(value).filter(k => !known.includes(k)).sort()
  if (extra.length > 0) throw new Error(`${place} に読まない欄がある: ${extra.join(', ')}`)
  return value as Json
}

/** オブジェクトでない値も「欄が無い」と読む (native の serde_json の Value::get と同じ) */
function field(value: unknown, name: string): unknown {
  if (value === null || typeof value !== 'object' || Array.isArray(value) || !Object.hasOwn(value, name)) {
    throw new Error(`欄 ${name} が無い`)
  }
  return (value as Json)[name]
}

function text(value: unknown, name: string): string {
  const v = field(value, name)
  if (typeof v !== 'string') throw new Error(`欄 ${name} が文字列でない`)
  return v
}

function list(value: unknown, name: string): unknown[] {
  const v = field(value, name)
  if (!Array.isArray(v)) throw new Error(`欄 ${name} が配列でない`)
  return v
}

function tickOf(value: unknown, name: string): number {
  const v = field(value, name)
  if (typeof v !== 'number' || !Number.isInteger(v) || v < 0 || v > 0xffffffff) throw new Error(`欄 ${name} が 0 以上の整数でない`)
  return v
}

function posOf(value: unknown, name: string): Pos {
  const v = field(value, name)
  const ok = (n: unknown) => typeof n === 'number' && Number.isInteger(n) && n >= -0x80000000 && n <= 0x7fffffff
  if (!Array.isArray(v) || v.length !== 3 || !v.every(ok)) throw new Error(`欄 ${name} が整数 3 つの配列でない`)
  return { x: v[0], y: v[1], z: v[2] }
}

/** fixture の JSON を読む。名前は wasm (rse_resolve) で解決する */
export function parseFixture(json: string, engine: Engine): Fixture {
  let parsed: unknown
  try {
    parsed = JSON.parse(json)
  } catch (e) {
    throw new Error(`JSON が読めない: ${(e as Error).message}`)
  }
  const root = onlyKnown(parsed, 'fixture', ROOT_KEYS)
  const region = onlyKnown(field(root, 'region'), 'region', REGION_KEYS)
  const [a, b] = [posOf(region, 'from'), posOf(region, 'to')]

  // 表に無いブロックは、全部集めてから断る (19 §3.1 の「未知」の段)。
  const blocks: Fixture['blocks'] = []
  const unknown: string[] = []
  for (const entry of list(root, 'blocks')) {
    const e = onlyKnown(entry, 'blocks の項目', BLOCK_KEYS)
    const name = text(e, 'block')
    const got = engine.tryResolve(name)
    if (got.ok) blocks.push({ pos: posOf(e, 'pos'), state: got.state })
    else if (!unknown.includes(name)) unknown.push(name)
  }
  if (unknown.length > 0) throw new Error(`状態表に無いブロック: ${unknown.join(', ')}`)

  const inputs: Fixture['inputs'] = []
  for (const e of list(root, 'inputs')) {
    const pos = posOf(e, 'pos')
    const action = text(e, 'action')
    // 右クリックはプレイヤーの操作 (区間 A。19 §4.1)。
    if (action !== 'use') throw new Error(`対応していない入力: ${action}`)
    onlyKnown(e, 'inputs の項目 (use)', USE_INPUT_KEYS)
    inputs.push({ tick: tickOf(e, 'tick'), section: 'A', pos })
  }

  const expect: Fixture['expect'] = []
  for (const entry of list(root, 'expect')) {
    const e = onlyKnown(entry, 'expect の項目', EXPECT_KEYS)
    const changes = list(e, 'changes').map(c => {
      const change = onlyKnown(c, 'changes の項目', CHANGE_KEYS)
      return { pos: posOf(change, 'pos'), block: text(change, 'block') }
    })
    expect.push({ tick: tickOf(e, 'tick'), changes })
  }

  return {
    name: text(root, 'name'),
    mcVersion: text(root, 'mcVersion'),
    ticks: tickOf(root, 'ticks'),
    from: { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), z: Math.min(a.z, b.z) },
    to: { x: Math.max(a.x, b.x), y: Math.max(a.y, b.y), z: Math.max(a.z, b.z) },
    blocks,
    inputs,
    expect,
  }
}

/**
 * tick `t` の入力を、適用する順で返す。1.21.1 で撮った fixture は書いてある並びのまま、
 * それ以外は区間の順 (C → A ほか) に安定に並べる (native の inputs_at と同じ。V01 §2.4)。
 */
function inputsAt(f: Fixture, t: number): Fixture['inputs'] {
  const now = f.inputs.filter(i => i.tick === t)
  if (!f.mcVersion.startsWith('1.21')) now.sort((p, q) => SECTION[p.section] - SECTION[q.section])
  return now
}

const key = (p: Pos) => `${p.x},${p.y},${p.z}`

function* region(f: Fixture): Generator<Pos> {
  for (let y = f.from.y; y <= f.to.y; y++) {
    for (let z = f.from.z; z <= f.to.z; z++) {
      for (let x = f.from.x; x <= f.to.x; x++) yield { x, y, z }
    }
  }
}

/** native と同じく fixture の並びで 1 マスずつ置き、最初に置けないマスを返す */
function firstUnplaceable(f: Fixture, engine: Engine): Pos | undefined {
  const probe = engine.createWorld()
  try {
    for (const b of f.blocks) {
      try {
        probe.load([{ ...b.pos, state: b.state }])
      } catch (e) {
        if (e instanceof EngineError && e.status & ABI_STATUS.OUT_OF_WORLD) return b.pos
        throw e
      }
    }
    return undefined
  } finally {
    probe.dispose()
  }
}

/** wasm の世界で回して、毎 tick の領域の全マスを期待と比べる */
export function runFixture(f: Fixture, engine: Engine): Report {
  const air = engine.states.name(0)
  const board = new Map<string, string>()
  for (const pos of region(f)) board.set(key(pos), air)
  for (const b of f.blocks) {
    // 領域の外のブロックは置くだけで、比べない。
    if (board.has(key(b.pos))) board.set(key(b.pos), engine.states.name(b.state))
  }

  const world = engine.createWorld()
  try {
    // 実際の盤面の写し。変化の窓だけで直す。
    const actual = new Map<string, number>()
    const sync = () => {
      const w = world.changes()
      for (let i = 0; i < w.count; i++) actual.set(`${w.x(i)},${w.y(i)},${w.z(i)}`, w.state(i))
    }
    try {
      const status = world.load(f.blocks.map(b => ({ ...b.pos, state: b.state })))
      if (status !== 0) throw new Error(`置いた時に状態コード ${statusNames(status).join(' | ')}`)
    } catch (e) {
      // load は全部か無しかなので、native と同じ文 (最初に置けないマスの座標) のために、1 マスずつ置き直して探す。
      const at = e instanceof EngineError && e.status & ABI_STATUS.OUT_OF_WORLD ? firstUnplaceable(f, engine) : undefined
      if (at === undefined) throw e
      throw new Error(`(${at.x}, ${at.y}, ${at.z}) に置けない (高さか格子の枠の外)`)
    }
    sync()

    const report: Report = { name: f.name, ticks: f.ticks, stepped: 0, compared: 0, mismatches: [], statuses: [] }
    for (let t = 0; t <= f.ticks; t++) {
      // 初めの盤面は tick −1 の状態。t = 0 では進めず、tick 0 の入力だけを入れる。
      let status = 0
      if (t > 0) {
        status |= world.step(1)
        report.stepped++
      }
      for (const input of inputsAt(f, t)) status |= world.use(input.pos.x, input.pos.y, input.pos.z, input.section)
      if (status !== 0) report.statuses.push([t, status])
      sync()
      for (const e of f.expect) {
        if (e.tick !== t) continue
        for (const c of e.changes) {
          if (!board.has(key(c.pos))) throw new Error(`tick ${t} の期待 (${c.pos.x}, ${c.pos.y}, ${c.pos.z}) が領域の外にある`)
          board.set(key(c.pos), c.block)
        }
      }
      for (const pos of region(f)) {
        const expect = board.get(key(pos)) as string
        const got = engine.states.name(actual.get(key(pos)) ?? 0)
        report.compared++
        if (expect !== got) report.mismatches.push({ tick: t, pos, expect, actual: got })
      }
    }
    return report
  } finally {
    world.dispose()
  }
}

export function runFixtureJson(json: string, engine: Engine): Report {
  return runFixture(parseFixture(json, engine), engine)
}
