import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { StructureProvider } from 'deepslate/core'
import { BoardModel, CHUNK_SIZE, MAX_CHUNK_LIST, cellKey, normalizeState, resourceNameOf, type Redraw } from './board-to-structure'
import { defaultPropsOf } from './renderer/buildResources'
import type { Board, BoardBounds, CellChange } from './types'

/**
 * 盤面から構造を作る所 (#11)。WebGL は要らない (deepslate の StructureProvider として見る)。
 *
 * 安全網 (redstone-sim の docs/research/20 §5.4): **差分で直した構造が、同じ盤面から丸ごと作った構造と同じ**になること。
 * 違うと、作り直さなかったマスが古い絵のまま残る。例外にならないので、テストでしか捕まえられない。
 */

const LEVER_OFF = 'lever[face=floor,facing=north,powered=false]'
const LEVER_ON = 'lever[face=floor,facing=north,powered=true]'
const WIRE = (p: number) => `redstone_wire[east=side,north=none,power=${p},south=none,west=side]`
const LAMP = (lit: boolean) => `redstone_lamp[lit=${lit}]`

const bounds = (min: [number, number, number], max: [number, number, number]): BoardBounds => ({
  x: [min[0], max[0]], y: [min[1], max[1]], z: [min[2], max[2]],
})
const boardOf = (b: BoardBounds, cells: Record<string, string> | Map<string, string>): Board => ({
  bounds: b,
  blocks: cells instanceof Map ? new Map(cells) : new Map(Object.entries(cells)),
})

/** 構造の中身: getBlocks の (構造ローカルの座標, deepslate の状態の文字列) の集合 */
function entries(s: StructureProvider): string[] {
  return s.getBlocks().map(b => `${b.pos.join(',')}=${b.state.toString()}`).sort()
}

/** 2 つの構造が同じか。getBlocks の集合と、範囲の全部のマスの getBlock (空は null) を比べる */
function expectSameStructure(a: StructureProvider, b: StructureProvider, label = ''): void {
  expect(a.getSize(), label).toEqual(b.getSize())
  expect(entries(a), label).toEqual(entries(b))
  const [sx, sy, sz] = a.getSize()
  for (let x = 0; x < sx; x++) {
    for (let y = 0; y < sy; y++) {
      for (let z = 0; z < sz; z++) {
        const pa = a.getBlock([x, y, z])?.state.toString() ?? null
        const pb = b.getBlock([x, y, z])?.state.toString() ?? null
        if (pa !== pb) expect(`${x},${y},${z}=${pa}`, label).toBe(`${x},${y},${z}=${pb}`)
      }
    }
  }
}

const DIRS = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]

/**
 * チャンクごとの「メッシュを決める物」。deepslate の ChunkBuilder は、ブロックの状態と、
 * 6 方向の面の隣の状態 (面の隠し。needsCull) からメッシュを作る。これが変わったチャンクは作り直しが要る
 */
function chunkSignatures(s: StructureProvider): Map<string, string> {
  const out = new Map<string, string[]>()
  for (const b of s.getBlocks()) {
    const c = b.pos.map(v => Math.floor(v / CHUNK_SIZE)).join(',')
    const around = DIRS.map(d => s.getBlock([b.pos[0] + d[0], b.pos[1] + d[1], b.pos[2] + d[2]])?.state.toString() ?? '-')
    const list = out.get(c) ?? []
    list.push(`${b.pos.join(',')}=${b.state.toString()}|${around.join('|')}`)
    out.set(c, list)
  }
  return new Map([...out].map(([k, v]) => [k, v.sort().join('\n')]))
}

/** 作り直しの前後で中身が変わったチャンクが、返ってきた一覧に入っているか */
function expectChunksCover(before: Map<string, string>, after: Map<string, string>, redraw: Redraw, label = ''): void {
  const changed = [...new Set([...before.keys(), ...after.keys()])].filter(k => before.get(k) !== after.get(k))
  if (redraw.kind === 'full') return
  if (redraw.kind === 'none') {
    expect(changed, `${label} 何も作り直さないのに中身が変わった`).toEqual([])
    return
  }
  const listed = new Set(redraw.chunks.map(c => c.join(',')))
  expect(changed.filter(k => !listed.has(k)), `${label} 作り直さなかったのに中身が変わったチャンク`).toEqual([])
}

/** 再現できる乱数 (mulberry32) */
function seeded(seed: number): () => number {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

describe('文字列の正規化 (normalizeState)', () => {
  it('先頭の minecraft: を外し、プロパティを名前順に並べ直す (StateTable.name の形)', () => {
    expect(normalizeState('minecraft:lever[powered=true,face=floor,facing=north]')).toBe(LEVER_ON)
    expect(normalizeState('redstone_wire[west=side,power=3,east=side,south=none,north=none]')).toBe(WIRE(3))
    // StateTable.name の形 (名前順で全部) はそのまま
    for (const s of [LEVER_OFF, WIRE(15), LAMP(true), 'stone']) expect(normalizeState(s)).toBe(s)
  })

  it('conformance の fixture に出る文字列 (実機の blockstate の書き方) は、どれもそのまま', () => {
    // fixture の文字列は StateTable.name と同じ書き方 (プロパティは名前順で全部)。並べ替えで崩さないことを、手で書いた例でなく実機の記録で見る
    const dir = new URL('../../../conformance/fixtures/', import.meta.url)
    const names = new Set<string>()
    for (const f of readdirSync(dir).filter(f => f.endsWith('.json'))) {
      const fx = JSON.parse(readFileSync(new URL(f, dir), 'utf-8')) as {
        blocks: { block: string }[]; expect: { changes: { block: string }[] }[]
      }
      for (const b of fx.blocks) names.add(b.block)
      for (const e of fx.expect) for (const c of e.changes) names.add(c.block)
    }
    const withProps = [...names].filter(n => n.includes('['))
    expect(withProps.length).toBeGreaterThan(5)
    for (const n of names) expect(normalizeState(n), n).toBe(n === 'air' ? null : n)
  })

  it('並べ替えは鍵で比べる (鍵が別の鍵の頭になる時も、`=` と数字の大小で崩れない)', () => {
    expect(normalizeState('x[powered=true,power=1]')).toBe('x[power=1,powered=true]')
    expect(normalizeState('x[a1=p,a=q]')).toBe('x[a=q,a1=p]')
  })

  it('air・cave_air・void_air は何も無いマス (null)', () => {
    for (const s of ['air', 'minecraft:air', 'cave_air', 'minecraft:void_air']) expect(normalizeState(s), s).toBeNull()
  })

  it('空の [] は外す。崩れた形とほかの名前空間はそのまま', () => {
    expect(normalizeState('minecraft:stone[]')).toBe('stone')
    expect(normalizeState('stone[a=1')).toBe('stone[a=1')
    expect(normalizeState('other:thing[b=1,a=2]')).toBe('other:thing[a=2,b=1]')
  })
})

describe('資源を作る名前の集め (#11 / redstone-sim#234)', () => {
  it('preloadNames は名前空間とプロパティを外し、air を除き、重ねない', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [0, 0, 0]), {}))
    expect(m.resourceNames(['minecraft:stone', 'stone', LEVER_OFF, 'minecraft:lever', 'air', 'cave_air', 'minecraft:void_air']))
      .toEqual(['lever', 'stone'])
  })

  it('盤面と変化に出た名前も集める。初めて出た名前だけを newNames で知らせる', () => {
    const b = bounds([0, 0, 0], [3, 0, 0])
    const m = new BoardModel(boardOf(b, { '0,0,0': 'minecraft:stone', '1,0,0': LEVER_OFF, '2,0,0': 'air' }))
    expect([...m.names].sort()).toEqual(['lever', 'stone'])
    const u1 = m.applyChanges([{ x: 1, y: 0, z: 0, state: LEVER_ON }, { x: 3, y: 0, z: 0, state: LAMP(false) }])
    expect(u1.newNames).toEqual(['redstone_lamp'])
    const u2 = m.setBoard(boardOf(b, { '0,0,0': 'minecraft:redstone_lamp[lit=true]', '1,0,0': 'glass' }))
    expect(u2.newNames).toEqual(['glass'])
    // 消えた名前も覚えたまま (資源は増えるだけ)
    expect(m.resourceNames()).toEqual(['glass', 'lever', 'redstone_lamp', 'stone'])
    expect(m.applyChanges([{ x: 2, y: 0, z: 0, state: 'stone' }]).newNames).toEqual([])
  })

  it('無染色のシュルカーボックスは紫で描く (deepslate に描き方が無く消えるため)。盤面の状態は読み替えない', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [1, 0, 0]), { '0,0,0': 'shulker_box[facing=up]', '1,0,0': 'lime_shulker_box[facing=up]' }))
    expect(resourceNameOf('minecraft:shulker_box')).toBe('purple_shulker_box')
    expect([...m.names].sort()).toEqual(['lime_shulker_box', 'purple_shulker_box'])
    expect(m.structure.getBlock([0, 0, 0])?.state.toString()).toBe('minecraft:purple_shulker_box[facing=up]')
    expect(m.stateAt(0, 0, 0)).toBe('shulker_box[facing=up]')
    expect(m.resourceNames(['shulker_box', 'chest'])).toEqual(['chest', 'lime_shulker_box', 'purple_shulker_box'])
  })
})

describe('盤面から構造を作る', () => {
  it('座標は範囲の最小を原点にした構造ローカル。範囲の外・空・air は null', () => {
    const b = bounds([-3, 64, 10], [1, 65, 11])
    const m = new BoardModel(boardOf(b, {
      [cellKey(-3, 64, 10)]: 'stone',
      [cellKey(-2, 65, 10)]: 'minecraft:lever[powered=false,face=floor,facing=north]',
      [cellKey(1, 65, 11)]: LAMP(false),
      [cellKey(0, 64, 11)]: 'minecraft:air',
    }))
    expect(m.structure.getSize()).toEqual([5, 2, 2])
    expect(m.min).toEqual([-3, 64, 10])
    expect(entries(m.structure)).toEqual([
      '0,0,0=minecraft:stone',
      `1,1,0=minecraft:${LEVER_OFF}`,
      `4,1,1=minecraft:${LAMP(false)}`,
    ].sort())
    expect(m.structure.getBlock([3, 0, 1])).toBeNull()         // air は何も無い
    expect(m.structure.getBlock([2, 0, 0])).toBeNull()         // 空
    for (const p of [[-1, 0, 0], [5, 0, 0], [0, 2, 0], [0, 0, -1]] as [number, number, number][]) {
      expect(m.structure.getBlock(p), p.join(',')).toBeNull()  // 範囲の外
    }
    expect(m.stateAt(-2, 65, 10)).toBe(LEVER_OFF)
    expect(m.stateAtLocal(1, 1, 0)).toBe(LEVER_OFF)
    expect(m.stateAt(0, 64, 11)).toBeNull()
  })

  it('範囲の外のマスと、鍵が "x,y,z" の形でないマスは描かずに数える (範囲の外の air は数えない)', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [1, 1, 1]), {}))
    const u = m.setBoard(boardOf(bounds([0, 0, 0], [1, 1, 1]), {
      '0,0,0': 'stone', '2,0,0': 'stone', '0,-1,0': LAMP(true), '5,5,5': 'air', 'a,b,c': 'stone', '1,1': 'stone', '1.5,0,0': 'stone',
    }))
    expect(u.outside).toBe(5)
    expect(u.changed).toBe(1)
    expect(entries(m.structure)).toEqual(['0,0,0=minecraft:stone'])
  })

  it('同じ文字列のマスは deepslate の状態を 1 つだけ作る (パレットは文字列で重複を除く)', () => {
    const cells: Record<string, string> = {}
    for (let x = 0; x < 4; x++) cells[cellKey(x, 0, 0)] = x % 2 === 0 ? 'stone' : 'minecraft:stone[]'
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [3, 0, 0]), cells))
    expect(new Set(m.structure.getBlocks().map(b => b.state)).size).toBe(1)
  })

  it('getBlocks は変わるまで同じ配列を返し、変わったら作り直す', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [1, 0, 0]), { '0,0,0': 'stone' }))
    const first = m.structure.getBlocks()
    expect(m.structure.getBlocks()).toBe(first)
    m.applyChanges([{ x: 1, y: 0, z: 0, state: 'stone' }])
    expect(m.structure.getBlocks()).not.toBe(first)
    expect(m.structure.getBlocks()).toHaveLength(2)
  })

  it('範囲が整数でなければ RangeError。最大が最小より小さい軸は大きさ 0', () => {
    expect(() => new BoardModel(boardOf({ x: [0, 1.5], y: [0, 0], z: [0, 0] }, {}))).toThrow(RangeError)
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [-1, 3, 3]), { '0,0,0': 'stone' }))
    expect(m.structure.getSize()).toEqual([0, 4, 4])
    expect(m.structure.getBlocks()).toEqual([])
  })
})

describe('差分で直した構造は、同じ盤面から丸ごと作った構造と同じ (20 §5.4 の安全網)', () => {
  it('lever-wire-lamp: レバーを入れた変化の列を当てると、点いた盤面を丸ごと作った物と同じ', () => {
    const b = bounds([0, 0, 0], [4, 1, 1])
    const initial: Record<string, string> = {}
    for (let x = 0; x <= 4; x++) for (let z = 0; z <= 1; z++) initial[cellKey(x, 0, z)] = 'stone'
    initial[cellKey(0, 1, 0)] = LEVER_OFF
    for (let x = 1; x <= 3; x++) initial[cellKey(x, 1, 0)] = WIRE(0)
    initial[cellKey(4, 1, 0)] = LAMP(false)
    const m = new BoardModel(boardOf(b, initial))

    // engine-js の変化の窓を StateTable.name で写した列の形 (t2 の use の後)
    const changes: CellChange[] = [
      { x: 0, y: 1, z: 0, state: LEVER_ON },
      { x: 1, y: 1, z: 0, state: WIRE(15) },
      { x: 2, y: 1, z: 0, state: WIRE(14) },
      { x: 3, y: 1, z: 0, state: WIRE(13) },
      { x: 4, y: 1, z: 0, state: LAMP(true) },
    ]
    const u = m.applyChanges(changes)
    expect(u).toMatchObject({ changed: 5, outside: 0, newNames: [] })
    // 5x2x2 は 1 チャンク。半分を超えるので全部を作り直す (大きさは同じ)
    expect(u.redraw).toEqual({ kind: 'full', resized: false })
    expect(m.stateAt(4, 1, 0)).toBe(LAMP(true))

    const lit = { ...initial }
    for (const c of changes) lit[cellKey(c.x, c.y, c.z)] = c.state
    expectSameStructure(m.structure, new BoardModel(boardOf(b, lit)).structure)
  })

  it('別の盤面から setBoard で差分を当てても、同じ盤面から丸ごと作った物と同じ', () => {
    const b = bounds([-2, 0, -2], [20, 4, 20])
    const a = new Map([[cellKey(0, 0, 0), 'stone'], [cellKey(5, 1, 5), LEVER_OFF], [cellKey(15, 2, 15), 'glass']])
    const next = new Map([[cellKey(0, 0, 0), 'minecraft:stone'], [cellKey(5, 1, 5), LEVER_ON], [cellKey(16, 2, 15), 'glass'], [cellKey(-2, 4, 20), LAMP(true)]])
    const m = new BoardModel(boardOf(b, a))
    const u = m.setBoard(boardOf(b, next))
    // stone は文字列がそろえば同じマス。lever は変わり、glass は動き、lamp は増えた
    expect(u.changed).toBe(4)
    expectSameStructure(m.structure, new BoardModel(boardOf(b, next)).structure)
    // 同じ中身の別の物を渡しても、何も作り直さない
    expect(m.setBoard(boardOf(b, next))).toMatchObject({ changed: 0, redraw: { kind: 'none' } })
  })

  it('種を固定した乱数の変化の列 (applyChanges・setBoard・範囲の変更を混ぜる) でも、毎回同じ', () => {
    const rnd = seeded(11)
    const pool = [
      'stone', 'minecraft:stone', 'glass', LEVER_OFF, LEVER_ON, 'lever[powered=true,facing=north,face=floor]',
      WIRE(0), WIRE(7), WIRE(15), LAMP(false), LAMP(true), 'air', 'minecraft:cave_air', 'shulker_box[facing=up]',
    ]
    const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(rnd() * xs.length)]
    const int = (lo: number, hi: number) => lo + Math.floor(rnd() * (hi - lo + 1))

    let b = bounds([-5, 0, 3], [34, 17, 38])        // 40 x 18 x 36: 3 x 2 x 3 = 18 チャンク
    const truth = new Map<string, string>()           // 今の盤面 (アプリが持つ写しの代わり)
    const inside = (x: number, y: number, z: number) =>
      x >= b.x[0] && x <= b.x[1] && y >= b.y[0] && y <= b.y[1] && z >= b.z[0] && z <= b.z[1]
    for (let i = 0; i < 400; i++) truth.set(cellKey(int(-5, 34), int(0, 17), int(3, 38)), pick(pool))
    const m = new BoardModel(boardOf(b, truth))
    const kinds = { chunks: 0, full: 0, none: 0, resized: 0 }

    for (let step = 0; step < 120; step++) {
      const before = chunkSignatures(m.structure)
      const r = rnd()
      let redraw: Redraw
      if (r < 0.75) {
        // 変化の列。たいていは数件で、たまに多い。範囲の外と同じマスの重なりも混ぜる
        const n = rnd() < 0.85 ? int(1, 6) : int(40, 120)
        const list: CellChange[] = []
        for (let i = 0; i < n; i++) {
          const near = rnd() < 0.4   // チャンクの境 (16 の倍数の前後) に寄せる
          const coord = (lo: number, hi: number) => (near ? Math.min(hi, lo + 16 * int(0, 2) + pick([-1, 0, 15, 16])) : int(lo - 2, hi + 2))
          list.push({ x: coord(b.x[0], b.x[1]), y: coord(b.y[0], b.y[1]), z: coord(b.z[0], b.z[1]), state: pick(pool) })
        }
        const u = m.applyChanges(list)
        let outside = 0
        for (const c of list) {
          if (!inside(c.x, c.y, c.z)) { outside++; continue }
          truth.set(cellKey(c.x, c.y, c.z), c.state)
        }
        expect(u.outside, `step ${step}`).toBe(outside)
        redraw = u.redraw
      } else if (r < 0.93) {
        // 盤面の差し替え (同じ範囲)。いくつかを変え、いくつかを消す
        for (let i = 0; i < int(1, 30); i++) {
          const k = cellKey(int(b.x[0], b.x[1]), int(b.y[0], b.y[1]), int(b.z[0], b.z[1]))
          if (rnd() < 0.3) truth.delete(k)
          else truth.set(k, pick(pool))
        }
        redraw = m.setBoard(boardOf(b, truth)).redraw
      } else {
        // 範囲を変える (広げるか縮める)。範囲の外になったマスは盤面から外す
        const grow = rnd() < 0.5 ? 3 : -3
        b = bounds([b.x[0] - grow, b.y[0], b.z[0]], [b.x[1] + grow, b.y[1], b.z[1] + grow])
        for (const k of [...truth.keys()]) {
          const [x, y, z] = k.split(',').map(Number)
          if (!inside(x, y, z)) truth.delete(k)
        }
        const u = m.setBoard(boardOf(b, truth))
        expect(u.redraw, `step ${step}`).toEqual({ kind: 'full', resized: true })
        redraw = u.redraw
        kinds.resized++
      }
      if (redraw.kind !== 'full' || !redraw.resized) {
        expectChunksCover(before, chunkSignatures(m.structure), redraw, `step ${step}`)
      }
      kinds[redraw.kind]++
      expectSameStructure(m.structure, new BoardModel(boardOf(b, truth)).structure, `step ${step}`)
      for (const [k, v] of truth) {
        const [x, y, z] = k.split(',').map(Number)
        expect(m.stateAt(x, y, z), k).toBe(normalizeState(v))
      }
    }
    // どの道も通っていること (偏ると確かめたことにならない)
    expect(kinds.chunks).toBeGreaterThan(30)
    expect(kinds.full - kinds.resized).toBeGreaterThan(3)
    expect(kinds.resized).toBeGreaterThan(3)
  })
})

describe('作り直すチャンクの一覧', () => {
  // 48 x 20 x 48: 3 x 2 x 3 = 18 チャンク (半分は 9)
  const b = bounds([100, 0, -50], [147, 19, -3])
  const at = (lx: number, ly: number, lz: number): CellChange => ({ x: 100 + lx, y: ly, z: -50 + lz, state: 'stone' })
  const listOf = (r: Redraw) => (r.kind === 'chunks' ? r.chunks.map(c => c.join(',')).sort() : r)

  it('チャンクの内側のマスは、そのチャンクだけ', () => {
    const m = new BoardModel(boardOf(b, {}))
    expect(listOf(m.applyChanges([at(20, 5, 40)]).redraw)).toEqual(['1,0,2'])
  })

  it('チャンクの面に接するマスは、その向こうのチャンクも入れる (x・y・z のどれでも、両側とも)', () => {
    const m = new BoardModel(boardOf(b, {}))
    expect(listOf(m.applyChanges([at(16, 5, 5)]).redraw)).toEqual(['0,0,0', '1,0,0'])
    expect(listOf(m.applyChanges([at(15, 5, 5)]).redraw)).toEqual(['0,0,0', '1,0,0'])
    expect(listOf(m.applyChanges([at(5, 16, 5)]).redraw)).toEqual(['0,0,0', '0,1,0'])
    expect(listOf(m.applyChanges([at(5, 5, 31)]).redraw)).toEqual(['0,0,1', '0,0,2'])
    // 角は面で接する 2 つ (斜めのチャンクは入れない。面の隠しは 6 方向の隣だけを見る)
    expect(listOf(m.applyChanges([at(16, 5, 32)]).redraw)).toEqual(['0,0,2', '1,0,1', '1,0,2'])
  })

  it('構造の端では、外側のチャンクを入れない', () => {
    const m = new BoardModel(boardOf(b, {}))
    expect(listOf(m.applyChanges([at(0, 0, 0)]).redraw)).toEqual(['0,0,0'])
    // y の最後は 19 (16 で割った余りは 3)、x の最後は 47 (余り 15 だが、その向こうは構造の外)
    expect(listOf(m.applyChanges([at(47, 19, 47)]).redraw)).toEqual(['2,1,2'])
  })

  it('チャンクが半分を超えたら全部を作り直す。範囲の外と、変わらないマスは数えない', () => {
    const m = new BoardModel(boardOf(b, {}))
    const nine = [0, 1, 2].flatMap(cx => [0, 1, 2].map(cz => at(cx * 16 + 5, 5, cz * 16 + 5)))
    expect(listOf(m.applyChanges(nine).redraw)).toHaveLength(9)
    const ten = [...[0, 1, 2].flatMap(cx => [0, 1, 2].map(cz => at(cx * 16 + 6, 5, cz * 16 + 6))), at(5, 17, 5)]
    expect(m.applyChanges(ten).redraw).toEqual({ kind: 'full', resized: false })
    // 同じ物をもう一度当てても何も変わらない
    expect(m.applyChanges(ten)).toMatchObject({ changed: 0, redraw: { kind: 'none' } })
    expect(MAX_CHUNK_LIST).toBe(64)
  })

  it('範囲の外の変化は描かずに数を返す (座標が整数でない物も)', () => {
    const m = new BoardModel(boardOf(b, {}))
    const u = m.applyChanges([
      { x: 99, y: 0, z: -50, state: 'stone' },
      { x: 100, y: 20, z: -50, state: 'stone' },
      { x: 100.5, y: 0, z: -50, state: 'stone' },
      { x: Number.NaN, y: 0, z: -50, state: 'stone' },
      at(3, 3, 3),
    ])
    expect(u).toMatchObject({ changed: 1, outside: 4 })
    expect(m.stateAt(99, 0, -50)).toBeNull()
    expect(entries(m.structure)).toEqual(['3,3,3=minecraft:stone'])
  })

  it('範囲が変わったら丸ごと作り直す (別の構造になる)', () => {
    const m = new BoardModel(boardOf(b, { [cellKey(100, 0, -50)]: 'stone' }))
    const old = m.structure
    const u = m.setBoard(boardOf(bounds([99, 0, -50], [147, 19, -3]), { [cellKey(100, 0, -50)]: 'stone' }))
    expect(u).toMatchObject({ changed: 0, redraw: { kind: 'full', resized: true } })
    expect(m.structure).not.toBe(old)
    expect(entries(m.structure)).toEqual(['1,0,0=minecraft:stone'])
  })
})

describe('読み込み中の変化 (redstone-sim#109)', () => {
  it('資源の準備ができる前に当てた変化と差し替えも、準備ができた時に引く構造に入っている', () => {
    // IsometricView は、資源を作り終えた時に model.structure から StructureRenderer を作る。
    // 移す元は effect が走った時の盤面から構造を作っていたので、この間の変化を落としていた
    const b = bounds([0, 0, 0], [4, 1, 0])
    const m = new BoardModel(boardOf(b, { '0,1,0': LEVER_OFF, '4,1,0': LAMP(false) }))
    const atStart = m.structure                                     // 読み込みを始めた時
    m.applyChanges([{ x: 0, y: 1, z: 0, state: LEVER_ON }])         // 読み込み中に押された
    m.setBoard(boardOf(b, { '0,1,0': LEVER_ON, '4,1,0': LAMP(true), '2,0,0': 'stone' }))
    const atReady = m.structure                                     // 準備ができた時
    expect(atReady).toBe(atStart)                                   // 範囲が同じなら同じ物を直している
    expectSameStructure(atReady, new BoardModel(boardOf(b, { '0,1,0': LEVER_ON, '4,1,0': LAMP(true), '2,0,0': 'stone' })).structure)
  })
})

describe('既定のプロパティ (buildResources の defaultPropsOf。redstone-sim#343)', () => {
  // 盤面の文字列がプロパティを省いていても、variant の値から補って描く (補わないと何も描かれず消える)
  it('既定プロパティは variant の値から作る (原木は縦置き・雪は無し)', () => {
    expect(defaultPropsOf({ variants: { 'axis=x': {}, 'axis=y': {}, 'axis=z': {} } }))
      .toEqual({ axis: 'y' })
    expect(defaultPropsOf({ variants: { 'snowy=false': {}, 'snowy=true': {} } }))
      .toEqual({ snowy: 'false' })
    expect(defaultPropsOf({ variants: { 'facing=east': {}, 'facing=north': {}, 'facing=south': {} } }))
      .toEqual({ facing: 'north' })
  })

  it('好ましい値が無ければ最初の値を使う', () => {
    expect(defaultPropsOf({ variants: { 'level=1': {}, 'level=2': {} } })).toEqual({ level: '1' })
  })

  it('プロパティ不問の variant があれば補わない', () => {
    expect(defaultPropsOf({ variants: { '': {} } })).toEqual({})
  })
})

describe('依存の向き (docs/architecture.md §1・§2)', () => {
  const srcDir = fileURLToPath(new URL('.', import.meta.url))
  const files = (readdirSync(srcDir, { recursive: true }) as string[])
    .filter(f => /\.tsx?$/.test(f))
    .map(f => ({ f, text: readFileSync(`${srcDir}/${f}`, 'utf-8') }))

  it('view-3d は engine-js・@redstone/sim・board を import しない', () => {
    const bad = files.filter(({ text }) => /from\s+'(@redstone-engine\/(engine-js|board)|@redstone\/)/.test(text)).map(x => x.f)
    expect(files.length).toBeGreaterThan(10)
    expect(bad).toEqual([])
  })

  it('窓口の型 (types.ts) は何も import せず、index.ts は deepslate の物を書き出さない', () => {
    const types = files.find(x => x.f === 'types.ts')?.text ?? ''
    const index = files.find(x => x.f === 'index.ts')?.text ?? ''
    expect(types).not.toMatch(/^import /m)
    expect(index).not.toMatch(/from\s+'deepslate/)
    expect(index).not.toMatch(/BoardModel|StructureProvider/)
  })
})
