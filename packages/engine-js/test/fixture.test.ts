import { beforeAll, describe, expect, it } from 'vitest'
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { Engine } from '../src/engine'
import { loadEngineFromFiles } from '../src/node'
import { formatReport, parseFixture, passed, runFixture, runFixtureJson } from './fixture'

/**
 * conformance/fixtures の 2 本を wasm で回す (#9)。native の実行器 (rse-cli) と同じく、毎 tick・領域の全マスで比べる。
 * 実機の正解は conformance/fixtures/SOURCES.md。expect は書き換えない (読んだ写しを変えるテストだけがある)。
 */

const root = new URL('../../../', import.meta.url)
const fixtureFile = (name: string) => new URL(`conformance/fixtures/${name}.json`, root)
const LEVER_WIRE_LAMP = readFileSync(fixtureFile('lever-wire-lamp'), 'utf-8')
const LAMP_OFF_DELAY = readFileSync(fixtureFile('lamp-off-delay'), 'utf-8')

let engine: Engine
beforeAll(async () => {
  engine = await loadEngineFromFiles()
})

describe('fixture を wasm で回す', () => {
  // レバー → ダスト 3 本 → ランプ。t2 に入れると、同じ tick にダストが 15・14・13 になり、ランプが点く。
  // [実機 fixture lever-wire-lamp]
  it('lever-wire-lamp が毎 tick 一致する', () => {
    const report = runFixtureJson(LEVER_WIRE_LAMP, engine)
    expect(passed(report), formatReport(report)).toBe(true)
    // tick 0〜8 の 9 回 × 領域 5 × 2 × 2 = 20 マス。
    expect(report.compared).toBe(9 * 20)
    expect(report.stepped).toBe(8)
  })

  // t2 に入れ、t10 に戻す。ダストは t10 に 0 へ戻り、ランプは 4gt 後の t14 に消える。
  // [実機 fixture lamp-off-delay]
  it('lamp-off-delay が毎 tick 一致する', () => {
    const report = runFixtureJson(LAMP_OFF_DELAY, engine)
    expect(passed(report), formatReport(report)).toBe(true)
    expect(report.compared).toBe(19 * 20)
    expect(report.stepped).toBe(18)
  })

  // native の実行器 (cargo run -p rse-cli -- fixture) と、報告の文が 1 字まで同じになる。
  it('native の実行器と同じ報告になる', () => {
    const files = ['lever-wire-lamp', 'lamp-off-delay'].map(n => fileURLToPath(fixtureFile(n)))
    const native = execFileSync('cargo', ['run', '-q', '-p', 'rse-cli', '--', 'fixture', ...files], {
      cwd: fileURLToPath(root),
      encoding: 'utf-8',
    })
    const wasm = [LEVER_WIRE_LAMP, LAMP_OFF_DELAY].map(json => formatReport(runFixtureJson(json, engine)))
    expect(native.trimEnd().split('\n')).toEqual(wasm)
  })
})

// 実行器が素通りしていないことを、読んだ写しを変えて確かめる (ファイルの expect には触れない)。
describe('wasm の実行器の数え方 (native と同じ)', () => {
  // 期待から t14 の消灯を抜くと、t14 から最後まで、ランプの 1 マスだけが合わない。
  it('合わない tick と座標を出す', () => {
    const fixture = parseFixture(LAMP_OFF_DELAY, engine)
    fixture.expect = fixture.expect.filter(e => e.tick !== 14)
    const report = runFixture(fixture, engine)
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

  // 入力を 1 tick 遅らせると、その tick の全部の変化が合わなくなる (入力の位相を見ている)。
  it('遅れた入力を見つける', () => {
    const fixture = parseFixture(LEVER_WIRE_LAMP, engine)
    fixture.inputs[0].tick = 3
    const report = runFixture(fixture, engine)
    expect(report.mismatches.every(m => m.tick === 2)).toBe(true)
    expect(report.mismatches.map(m => m.pos.x)).toEqual([0, 1, 2, 3, 4])
  })

  // 初めの盤面は tick −1 の状態で、t = 0 では step を呼ばない。t0 に入れて ticks = 0 なら、1 回も進めずに比べる。
  it('t = 0 では step を呼ばない', () => {
    const fixture = parseFixture(LEVER_WIRE_LAMP, engine)
    fixture.inputs[0].tick = 0
    fixture.expect[0].tick = 0
    fixture.ticks = 0
    const report = runFixture(fixture, engine)
    expect(passed(report), formatReport(report)).toBe(true)
    expect(report.stepped).toBe(0)
    expect(report.compared).toBe(20)
  })

  // 19 §4.2 の点検: 初めの盤面を置き、入力なしで 8 tick 空回ししても何も変わらない。
  it('初めの盤面は 8 tick 静か', () => {
    for (const json of [LEVER_WIRE_LAMP, LAMP_OFF_DELAY]) {
      const fixture = parseFixture(json, engine)
      fixture.inputs = []
      fixture.expect = []
      fixture.ticks = 8
      const report = runFixture(fixture, engine)
      expect(passed(report), formatReport(report)).toBe(true)
      expect(report.compared).toBe(9 * 20)
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
