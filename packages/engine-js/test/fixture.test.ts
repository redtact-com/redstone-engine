import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { Engine } from '../src/engine'
import { loadEngineFromFiles } from '../src/node'
import { type Build, formatReport, parseFixture, passed, runFixture, runFixtureJson } from './fixture'

/**
 * conformance/fixtures の全部を wasm で回す (#9)。native の実行器 (rse-cli) と同じく、毎 tick・領域の全マスで比べる。
 * 実機の正解は conformance/fixtures/SOURCES.md。expect は書き換えない (読んだ写しを変えるテストだけがある)。
 */

const root = fileURLToPath(new URL('../../../', import.meta.url))
const fixtureDir = join(root, 'conformance', 'fixtures')
const FIXTURES = readdirSync(fixtureDir)
  .filter(f => f.endsWith('.json'))
  .sort()
  .map(f => ({ name: f.replace(/\.json$/, ''), path: join(fixtureDir, f), json: readFileSync(join(fixtureDir, f), 'utf-8') }))
const fixture = (name: string) => FIXTURES.find(f => f.name === name)?.json ?? ''
const LEVER_WIRE_LAMP = fixture('lever-wire-lamp')
const LAMP_OFF_DELAY = fixture('lamp-off-delay')

/** 0〜ticks */
const ticksOf = (ticks: number) => Array.from({ length: ticks + 1 }, (_, t) => t)

/** 0〜n−1 の並べ替えの全部 */
function permutations(n: number): number[][] {
  if (n === 0) return [[]]
  return permutations(n - 1).flatMap(rest => Array.from({ length: n }, (_, at) => rest.toSpliced(at, 0, n - 1)))
}

/** native の実行器に JSON を渡して回す。ファイルは OS の一時フォルダに書き、終わったら消す */
const work = mkdtempSync(join(tmpdir(), 'rse-fixture-'))
afterAll(() => rmSync(work, { recursive: true, force: true }))
function native(files: string[]): { status: number | null; stdout: string; stderr: string } {
  const run = spawnSync('cargo', ['run', '-q', '-p', 'rse-cli', '--', 'fixture', ...files], { cwd: root, encoding: 'utf-8' })
  return { status: run.status, stdout: run.stdout, stderr: run.stderr }
}
function nativeJson(name: string, json: string): ReturnType<typeof native> {
  const file = join(work, `${name}.json`)
  writeFileSync(file, json)
  return native([file])
}

let engine: Engine
beforeAll(async () => {
  engine = await loadEngineFromFiles()
})

describe('fixture を wasm で回す', () => {
  it('conformance/fixtures に 2 本以上ある', () => {
    expect(FIXTURES.map(f => f.name)).toEqual(expect.arrayContaining(['lamp-off-delay', 'lever-wire-lamp']))
  })

  // lever-wire-lamp: レバー → ダスト 3 本 → ランプ。t2 に入れると、同じ tick にダストが 15・14・13 になり、ランプが点く。
  // lamp-off-delay: 同じ回路。t2 に入れ、t10 に戻す。ダストは t10 に 0 へ戻り、ランプは 4gt 後の t14 に消える。
  // [実機 fixture。SOURCES.md]
  for (const f of FIXTURES) {
    it(`${f.name} が毎 tick 一致する`, () => {
      const report = runFixtureJson(f.json, engine)
      expect(passed(report), formatReport(report)).toBe(true)
      // step を呼ぶのは t ≥ 1 だけ。比べるのは tick 0〜ticks の毎回、領域の全マス。
      expect(report.stepped).toBe(report.ticks)
      expect(report.compared % (report.ticks + 1)).toBe(0)
      // 世界の時計 (L5 のゲーム時刻) は fixture の t と同じ。初めの盤面 (tick −1 の状態) を置いた後は 0 で、
      // t = 0 では進めず、t ≥ 1 で 1 つずつ進む。入力 (t2 などの use) では進まない。
      expect(report.times).toEqual(ticksOf(report.ticks))
    })
  }

  it('数は native のテストと同じ', () => {
    // tick 0〜8 の 9 回 × 領域 5 × 2 × 2 = 20 マス。lamp-off-delay は tick 0〜18 の 19 回。
    expect(runFixtureJson(LEVER_WIRE_LAMP, engine).compared).toBe(9 * 20)
    expect(runFixtureJson(LAMP_OFF_DELAY, engine).compared).toBe(19 * 20)
  })

  // native の実行器 (cargo run -p rse-cli -- fixture) に全部を渡し、報告の文が 1 字まで同じになる。
  it('native の実行器と同じ報告になる', () => {
    const run = native(FIXTURES.map(f => f.path))
    expect(run.status, run.stderr).toBe(0)
    const wasm = FIXTURES.map(f => formatReport(runFixtureJson(f.json, engine)))
    expect(run.stdout.trimEnd().split('\n')).toEqual(wasm)
  })

  // 合わない時も、報告の全文 (不一致の見出しと不一致の行) が native と同じになる。
  // 状態コードの行は、今の 2 本の回路では立たないので比べていない (状態コードを立てる fixture が入ったら足す)。
  it('合わない fixture でも native と同じ報告になる', () => {
    const json = JSON.parse(LAMP_OFF_DELAY)
    json.expect = json.expect.filter((e: { tick: number }) => e.tick !== 14)
    const text = JSON.stringify(json)
    const run = nativeJson('no-t14', text)
    expect(run.status).toBe(1)
    const report = runFixtureJson(text, engine)
    expect(passed(report)).toBe(false)
    expect(run.stdout.trimEnd()).toBe(formatReport(report))
  })

  // 断る時の文も native と同じ (置けないマス・オブジェクトでない入力・読まない欄・表に無いブロック)。
  it('断る時の文が native と同じ', () => {
    const base = JSON.parse(LEVER_WIRE_LAMP)
    const cases: [string, (j: Record<string, unknown[]>) => void][] = [
      ['too-high', j => j.blocks.push({ pos: [0, 400, 0], block: 'stone' })],
      ['too-far', j => j.blocks.push({ pos: [4096, 0, 0], block: 'stone' }, { pos: [9000, 0, 0], block: 'stone' })],
      ['not-object', j => j.inputs.push(1)],
      ['unknown-block', j => j.blocks.push({ pos: [0, 2, 0], block: 'repeater' })],
      ['unread', j => j.inputs.push({ tick: 1, pos: [0, 1, 0], action: 'use', face: 'up' })],
    ]
    for (const [name, edit] of cases) {
      const json = structuredClone(base)
      edit(json)
      const text = JSON.stringify(json)
      const run = nativeJson(name, text)
      expect(run.status, name).toBe(1)
      const message = run.stderr.trimEnd().replace(/^.*?\.json: /, '')
      expect(() => runFixtureJson(text, engine), name).toThrow(message)
      let thrown = ''
      try {
        runFixtureJson(text, engine)
      } catch (e) {
        thrown = (e as Error).message
      }
      expect(thrown, name).toBe(message)
    }
  })
})

// 実行器が素通りしていないことを、読んだ写しを変えて確かめる (ファイルの expect には触れない)。
describe('wasm の実行器の数え方 (native と同じ)', () => {
  // 期待から t14 の消灯を抜くと、t14 から最後まで、ランプの 1 マスだけが合わない。
  it('合わない tick と座標を出す', () => {
    const f = parseFixture(LAMP_OFF_DELAY, engine)
    f.expect = f.expect.filter(e => e.tick !== 14)
    const report = runFixture(f, engine)
    expect(passed(report)).toBe(false)
    expect(report.mismatches.map(m => m.tick)).toEqual([14, 15, 16, 17, 18])
    for (const m of report.mismatches) {
      expect(m.pos).toEqual({ x: 4, y: 1, z: 0 })
      expect(m.expect).toBe('redstone_lamp[lit=true]')
      expect(m.actual).toBe('redstone_lamp[lit=false]')
    }
    expect(formatReport(report)).toContain(
      'tick 14 (4, 1, 0): 期待 redstone_lamp[lit=true] / 実際 redstone_lamp[lit=false]',
    )
  })

  // 入力を 1 tick 遅らせると、その tick の全部の変化 (レバー・ダスト 3 本・ランプ) が合わなくなる (入力の位相を見ている)。
  it('遅れた入力を見つける', () => {
    const f = parseFixture(LEVER_WIRE_LAMP, engine)
    f.inputs[0].tick = 3
    const report = runFixture(f, engine)
    expect(report.mismatches.map(m => [m.tick, m.pos.x, m.pos.y, m.pos.z])).toEqual(
      [0, 1, 2, 3, 4].map(x => [2, x, 1, 0]),
    )
  })

  // 初めの盤面は tick −1 の状態で、t = 0 では step を呼ばない。t0 に入れて ticks = 0 なら、1 回も進めずに比べる。
  it('t = 0 では step を呼ばない', () => {
    const f = parseFixture(LEVER_WIRE_LAMP, engine)
    f.inputs[0].tick = 0
    f.expect[0].tick = 0
    f.ticks = 0
    const report = runFixture(f, engine)
    expect(passed(report), formatReport(report)).toBe(true)
    expect(report.stepped).toBe(0)
    expect(report.compared).toBe(20)
  })

  // 19 §4.2 の点検: 初めの盤面を置き、入力なしで 8 tick 空回ししても何も変わらない。
  it('初めの盤面は 8 tick 静か', () => {
    for (const { json } of FIXTURES) {
      const f = parseFixture(json, engine)
      f.inputs = []
      f.expect = []
      f.ticks = 8
      const report = runFixture(f, engine)
      expect(passed(report), formatReport(report)).toBe(true)
      expect(report.stepped).toBe(8)
    }
  })

  it('読まない欄があれば断る', () => {
    const run = (from: string, to: string) => {
      expect(LEVER_WIRE_LAMP).toContain(from)
      return () => runFixtureJson(LEVER_WIRE_LAMP.replace(from, to), engine)
    }
    expect(run('"ticks": 8,', '"ticks": 8, "trustAuthored": true, "skipUntil": 3,')).toThrow(
      'fixture に読まない欄がある: skipUntil, trustAuthored',
    )
    expect(run('"block": "stone"', '"block": "stone", "items": []')).toThrow('blocks の項目 に読まない欄がある: items')
    expect(run('"action": "use"', '"action": "use", "face": "up"')).toThrow('inputs の項目 (use) に読まない欄がある: face')
    for (const extra of ['scheduled', 'comparators', 'cooldowns', 'skipReason']) {
      expect(run('"ticks": 8,', `"ticks": 8, "${extra}": [],`)).toThrow(`fixture に読まない欄がある: ${extra}`)
    }
  })

  it('表に無いブロックと知らない入力は断る', () => {
    const base = (blocks: string, inputs: string) =>
      `{"name":"x","mcVersion":"26.3","ticks":1,"region":{"from":[0,0,0],"to":[0,0,0]},"blocks":${blocks},"inputs":${inputs},"expect":[]}`
    expect(() =>
      runFixtureJson(base('[{"pos":[0,0,0],"block":"repeater"},{"pos":[0,1,0],"block":"piston"}]', '[]'), engine),
    ).toThrow('状態表に無いブロック: repeater, piston')
    expect(() => runFixtureJson(base('[]', '[{"tick":0,"pos":[0,0,0],"action":"place"}]'), engine)).toThrow(
      '対応していない入力: place',
    )
    expect(passed(runFixtureJson(base('[]', '[]'), engine))).toBe(true)
    expect(() => runFixtureJson('{', engine)).toThrow('JSON が読めない')
  })
})

// 初めの盤面を、置く入力 (RustWorld.place) で組む。native の rse-cli の試験と同じ組み方を、本物の wasm で回す。
// fixture 2 本とも、床 (y=0) の 10 マスを更新なしで置いてから、上 (y=1) の 5 個 (レバー・ダスト 3 本・ランプ) を
// 120 通りの順で置く。ダストとランプの要求は既定の状態で、形と lit は置いた時の規則と形状更新が決める。
// レバーは face と facing を運ぶので、fixture の状態のまま渡す。
// - どの順でも、置く入力は 0 を返し、組んだ後の時計は 0 のまま。
// - そこから fixture の入力で回すと、expect と毎 tick 全マスが一致する。tick 0 の比べは初めの盤面そのもの
//   (2 本とも、tick 0 の入力と期待は無い)。時計は毎 tick fixture の t と同じ。
// fixture のファイルは変えない。入力の action に置くを足さない (実機のハーネスに無い。断る試験はそのまま)。
describe('初めの盤面を置く入力で組む', () => {
  for (const name of ['lever-wire-lamp', 'lamp-off-delay']) {
    it(`${name}: 上の 5 個を 120 通りの順で置いて組み、そこから毎 tick 一致する`, () => {
      const f = parseFixture(fixture(name), engine)
      expect(f.inputs.every(i => i.tick > 0) && f.expect.every(e => e.tick > 0)).toBe(true)
      const floor = f.blocks.filter(b => b.pos.y === 0)
      const upper = f.blocks.filter(b => b.pos.y !== 0)
      expect([floor.length, upper.length]).toEqual([10, 5])
      const orders = permutations(upper.length)
      expect(orders).toHaveLength(120)
      expect(new Set(orders.map(o => o.join())).size).toBe(120)
      for (const order of orders) {
        const build: Build = world => {
          expect(world.load(floor.map(b => ({ ...b.pos, state: b.state })))).toBe(0)
          for (const i of order) {
            const { pos, state } = upper[i]
            const block = engine.states.block(state)
            // 周りで決まる値を捨てた要求にする。レバーは face と facing を運ぶので、そのまま。
            const requested = block.behavior === 'Lever' ? state : block.defaultState
            expect(world.place(pos.x, pos.y, pos.z, requested), `${name} ${order}`).toBe(0)
          }
          expect(world.gameTime()).toBe(0)
        }
        const report = runFixture(f, engine, build)
        expect(passed(report), `${name} ${order}\n${formatReport(report)}`).toBe(true)
        expect(report.times).toEqual(ticksOf(f.ticks))
      }
    })
  }

  // 素通りしていない: ダストを置かずに組むと、tick 0 (初めの盤面) から合わない。
  it('置き損ねた盤面は tick 0 から合わない', () => {
    const f = parseFixture(LEVER_WIRE_LAMP, engine)
    const report = runFixture(f, engine, world => {
      world.load(f.blocks.filter(b => b.pos.y === 0).map(b => ({ ...b.pos, state: b.state })))
      for (const { pos, state } of f.blocks.filter(b => b.pos.y !== 0 && engine.states.block(b.state).behavior !== 'Wire')) {
        world.place(pos.x, pos.y, pos.z, state)
      }
    })
    expect(report.mismatches.filter(m => m.tick === 0).map(m => m.pos.x)).toEqual([1, 2, 3])
  })
})
