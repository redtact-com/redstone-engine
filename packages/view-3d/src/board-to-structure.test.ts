import { describe, it, expect } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import type { BlockPos, StructureProvider } from 'deepslate/core'
import {
  BoardModel, BoardView, CHUNK_SIZE, MAX_CHUNK_LIST, cellKey, normalizeState, resourceNameOf,
  type Redraw, type RedrawTarget, type ViewHooks,
} from './board-to-structure'
import { releaseWhenDetached, type WatchDom } from './IsometricView'
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

  it('範囲の外のマスと、鍵が "x,y,z" の形でないマスは描かずに数える (範囲の外の air は数えない・崩れた鍵は air でも数える)', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [1, 1, 1]), {}))
    const u = m.setBoard(boardOf(bounds([0, 0, 0], [1, 1, 1]), {
      '0,0,0': 'stone', '2,0,0': 'stone', '0,-1,0': LAMP(true), '5,5,5': 'air', 'a,b,c': 'stone', '1,1': 'stone', '1.5,0,0': 'stone',
      // 鍵の各部分は /^-?\d+$/ (空白・+・指数・空は崩れた形)
      ' 1,0,0': 'stone', '+1,0,0': 'stone', '1e0,0,0': 'stone', '1,0,0 ': 'stone', ',0,0': 'stone', '1,0,0,': 'air',
    }))
    expect(u.outside).toBe(11)
    expect(u.changed).toBe(1)
    expect(entries(m.structure)).toEqual(['0,0,0=minecraft:stone'])
    // 符号と頭の 0 は整数の字面
    expect(m.setBoard(boardOf(bounds([-1, 0, 0], [1, 1, 1]), { '-1,0,0': 'stone', '01,0,0': 'glass' })).outside).toBe(0)
    expect(m.stateAt(1, 0, 0)).toBe('glass')
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

  it('盤面が途中で投げても (文字列でない値・範囲の誤り)、描く盤面は変わらない', () => {
    const b = bounds([0, 0, 0], [3, 0, 0])
    const m = new BoardModel(boardOf(b, { '0,0,0': 'stone' }))
    const before = entries(m.structure)
    const bad = new Map<string, string>([['1,0,0', 'glass'], ['2,0,0', 42 as unknown as string]])
    expect(() => m.setBoard({ bounds: b, blocks: bad })).toThrow(TypeError)
    expect(() => m.setBoard(boardOf({ x: [0, 0.5], y: [0, 0], z: [0, 0] }, { '1,0,0': 'glass' }))).toThrow(RangeError)
    expect(entries(m.structure)).toEqual(before)
    expect(m.stateAt(1, 0, 0)).toBeNull()
    expect([...m.names]).toEqual(['stone'])
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

  it('種を固定した乱数の変化の列 (applyChanges・setBoard・範囲の変更を混ぜる) でも、毎回同じ。changed と newNames も正しい', () => {
    const rnd = seeded(11)
    const pool = [
      'stone', 'minecraft:stone', 'glass', LEVER_OFF, LEVER_ON, 'lever[powered=true,facing=north,face=floor]',
      WIRE(0), WIRE(7), WIRE(15), LAMP(false), LAMP(true), 'air', 'minecraft:cave_air', 'shulker_box[facing=up]',
      'redstone_torch[lit=true]', 'iron_bars[east=false,north=false,south=false,waterlogged=false,west=false]',
    ]
    // 初めの盤面には pool の一部だけを置く (新しい名前の道を通すため)
    const firstPool = pool.slice(0, 4)
    // 途中で pool に足す名前と、範囲が変わる歩で 1 つずつ置く名前 (範囲が変わる道でも新しい名前を通す)
    const later = new Map<number, string>([[50, 'observer[facing=north,powered=false]'], [100, 'target[power=0]']])
    const forResize = ['note_block[instrument=harp,note=0,powered=false]', 'piston[extended=false,facing=up]', 'slime_block', 'hopper[enabled=true,facing=down]', 'dropper[facing=up,triggered=false]', 'composter[level=0]', 'lodestone', 'soul_sand']
    const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(rnd() * xs.length)]
    const int = (lo: number, hi: number) => lo + Math.floor(rnd() * (hi - lo + 1))

    let b = bounds([-5, 0, 3], [34, 17, 38])        // 40 x 18 x 36: 3 x 2 x 3 = 18 チャンク
    const truth = new Map<string, string>()           // 今の盤面 (アプリが持つ写しの代わり)
    const inside = (x: number, y: number, z: number) =>
      x >= b.x[0] && x <= b.x[1] && y >= b.y[0] && y <= b.y[1] && z >= b.z[0] && z <= b.z[1]
    /** 盤面を正規化した表 (何も無いマスは持たない) */
    const normalizedOf = (t: Map<string, string>) => {
      const out = new Map<string, string>()
      for (const [k, v] of t) {
        const n = normalizeState(v)
        if (n !== null) out.set(k, n)
      }
      return out
    }
    for (let i = 0; i < 400; i++) truth.set(cellKey(int(-5, 34), int(0, 17), int(3, 38)), pick(firstPool))
    const m = new BoardModel(boardOf(b, truth))
    const seen = new Set(m.names)
    expect([...seen].sort()).toEqual(['glass', 'lever', 'stone'])
    const kinds = { chunks: 0, full: 0, none: 0, resized: 0, named: 0, resizedNamed: 0, lateNamed: 0 }

    for (let step = 0; step < 160; step++) {
      const added = later.get(step)
      if (added !== undefined) pool.push(added)
      const before = chunkSignatures(m.structure)
      const prev = normalizedOf(truth)
      const r = rnd()
      let u: ReturnType<BoardModel['applyChanges']>
      if (r < 0.72) {
        // 変化の列。たいていは数件で、たまに多い。範囲の外と同じマスの重なりも混ぜる
        const n = rnd() < 0.85 ? int(1, 6) : int(40, 120)
        const list: CellChange[] = []
        for (let i = 0; i < n; i++) {
          const near = rnd() < 0.4   // チャンクの境 (16 の倍数の前後) に寄せる
          const coord = (lo: number, hi: number) => (near ? Math.min(hi, lo + 16 * int(0, 2) + pick([-1, 0, 15, 16])) : int(lo - 2, hi + 2))
          list.push({ x: coord(b.x[0], b.x[1]), y: coord(b.y[0], b.y[1]), z: coord(b.z[0], b.z[1]), state: pick(pool) })
        }
        u = m.applyChanges(list)
        let outside = 0
        for (const c of list) {
          if (!inside(c.x, c.y, c.z)) {
            if (normalizeState(c.state) !== null) outside++
            continue
          }
          truth.set(cellKey(c.x, c.y, c.z), c.state)
        }
        expect(u.outside, `step ${step}`).toBe(outside)
      } else if (r < 0.9) {
        // 盤面の差し替え (同じ範囲)。いくつかを変え、いくつかを消す
        const n = int(1, 30)
        for (let i = 0; i < n; i++) {
          const k = cellKey(int(b.x[0], b.x[1]), int(b.y[0], b.y[1]), int(b.z[0], b.z[1]))
          if (rnd() < 0.3) truth.delete(k)
          else truth.set(k, pick(pool))
        }
        u = m.setBoard(boardOf(b, truth))
      } else {
        // 範囲を変える (広げるか縮める)。範囲の外になったマスは盤面から外し、新しい範囲にいくつか置く
        const grow = rnd() < 0.5 ? 3 : -3
        b = bounds([b.x[0] - grow, b.y[0], b.z[0]], [b.x[1] + grow, b.y[1], b.z[1] + grow])
        for (const k of [...truth.keys()]) {
          const [x, y, z] = k.split(',').map(Number)
          if (!inside(x, y, z)) truth.delete(k)
        }
        const n = int(1, 3)
        for (let i = 0; i < n; i++) truth.set(cellKey(int(b.x[0], b.x[1]), int(b.y[0], b.y[1]), int(b.z[0], b.z[1])), pick(pool))
        const fresh = forResize.shift()
        if (fresh !== undefined) truth.set(cellKey(int(b.x[0], b.x[1]), int(b.y[0], b.y[1]), int(b.z[0], b.z[1])), fresh)
        u = m.setBoard(boardOf(b, truth))
        expect(u.redraw, `step ${step}`).toEqual({ kind: 'full', resized: true })
        kinds.resized++
        if (u.newNames.length > 0) kinds.resizedNamed++
      }

      // 数と名前: 正しい値 (前後の盤面の違い・初めて出た名前) と比べる
      const next = normalizedOf(truth)
      let changed = 0
      for (const k of new Set([...prev.keys(), ...next.keys()])) if (prev.get(k) !== next.get(k)) changed++
      const fresh = [...new Set([...next.values()].map(v => resourceNameOf(v) as string))].filter(n => !seen.has(n)).sort()
      fresh.forEach(n => seen.add(n))
      expect(u.changed, `step ${step} changed`).toBe(changed)
      expect([...u.newNames].sort(), `step ${step} newNames`).toEqual(fresh)
      if (fresh.length > 0) {
        kinds.named++
        // 途中で足した名前が、範囲の変わらない道 (applyChanges・setBoard) で初めて出た
        if (step >= 50 && !(u.redraw.kind === 'full' && u.redraw.resized)) kinds.lateNamed++
      }

      if (u.redraw.kind !== 'full' || !u.redraw.resized) {
        expectChunksCover(before, chunkSignatures(m.structure), u.redraw, `step ${step}`)
      }
      kinds[u.redraw.kind]++
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
    expect(kinds.lateNamed).toBeGreaterThan(0)                     // 範囲が変わらない道でも、途中で名前が出た
    expect(kinds.resizedNamed).toBeGreaterThan(0)                  // 範囲が変わる道でも
    for (const n of ['iron_bars', 'purple_shulker_box', 'redstone_lamp', 'redstone_torch', 'redstone_wire', 'observer', 'target']) {
      expect(seen.has(n), n).toBe(true)
    }
  })

  it('範囲を広げると同時に新しい名前を置くと、newNames と changed を返し、そのマスを描く', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [3, 0, 0]), { '0,0,0': 'stone', '3,0,0': LEVER_OFF }))
    const u = m.setBoard(boardOf(bounds([0, 0, 0], [7, 2, 0]), {
      '0,0,0': 'stone', '3,0,0': LEVER_ON, '6,2,0': LAMP(true), '7,0,0': 'minecraft:glass',
    }))
    expect(u.redraw).toEqual({ kind: 'full', resized: true })
    expect([...u.newNames].sort()).toEqual(['glass', 'redstone_lamp'])
    expect(u.changed).toBe(3)   // レバーが変わり、ランプとガラスが増えた (石はそのまま)
    expect(m.structure.getSize()).toEqual([8, 3, 1])
    expect(m.structure.getBlock([6, 2, 0])?.state.toString()).toBe(`minecraft:${LAMP(true)}`)
    // 縮めると、外に出たマスは消えたと数える
    const v = m.setBoard(boardOf(bounds([0, 0, 0], [3, 0, 0]), { '0,0,0': 'stone', '3,0,0': LEVER_ON }))
    expect(v).toMatchObject({ changed: 2, newNames: [], redraw: { kind: 'full', resized: true } })
  })
})

describe('applyChanges は列を読み切ってから当てる', () => {
  const b = bounds([0, 0, 0], [40, 3, 40])   // 3 x 1 x 3 チャンク
  const start = () => new BoardModel(boardOf(b, { '0,0,0': 'stone', '20,0,0': LEVER_OFF }))

  it('列が途中で投げたら、描く盤面は変わらない。同じ列を当て直すと、作り直す物と新しい名前が返る', () => {
    const m = start()
    const before = entries(m.structure)
    function* broken(): Generator<CellChange> {
      yield { x: 20, y: 0, z: 0, state: LEVER_ON }
      yield { x: 5, y: 1, z: 1, state: LAMP(true) }
      throw new Error('列の途中で失敗')
    }
    expect(() => m.applyChanges(broken())).toThrow('列の途中で失敗')
    expect(entries(m.structure)).toEqual(before)
    expect(m.stateAt(20, 0, 0)).toBe(LEVER_OFF)
    expect([...m.names].sort()).toEqual(['lever', 'stone'])

    // 当て直すと、当たっていなかったので全部が変化として返る
    const u = m.applyChanges([{ x: 20, y: 0, z: 0, state: LEVER_ON }, { x: 5, y: 1, z: 1, state: LAMP(true) }])
    expect(u).toMatchObject({ changed: 2, newNames: ['redstone_lamp'] })
    expect(u.redraw.kind).toBe('chunks')
  })

  it('文字列でない状態があれば TypeError で、どのマスも当てない', () => {
    const m = start()
    const before = entries(m.structure)
    expect(() => m.applyChanges([
      { x: 1, y: 0, z: 0, state: 'glass' },
      { x: 2, y: 0, z: 0, state: null as unknown as string },
    ])).toThrow(TypeError)
    expect(entries(m.structure)).toEqual(before)
  })

  it('同じマスは最後の状態だけを当て、マスの数で返す (A → B → A なら何もしない。一時の名前で資源を作り直さない)', () => {
    const m = start()
    expect(m.applyChanges([
      { x: 20, y: 0, z: 0, state: LAMP(true) },
      { x: 20, y: 0, z: 0, state: 'minecraft:lever[powered=false,face=floor,facing=north]' },
    ])).toMatchObject({ changed: 0, newNames: [], redraw: { kind: 'none' } })
    expect(m.names.has('redstone_lamp')).toBe(false)
    const u = m.applyChanges([
      { x: 20, y: 0, z: 0, state: LEVER_ON },
      { x: 20, y: 0, z: 0, state: LEVER_OFF },
      { x: 20, y: 0, z: 0, state: LEVER_ON },
    ])
    expect(u.changed).toBe(1)
    expect(m.stateAt(20, 0, 0)).toBe(LEVER_ON)
  })

  it('範囲の外は件数で数える。範囲の外の air は数えず、座標が崩れた物は air でも数える', () => {
    const m = start()
    const u = m.applyChanges([
      { x: 41, y: 0, z: 0, state: 'stone' },
      { x: 41, y: 0, z: 0, state: 'stone' },
      { x: 0, y: 4, z: 0, state: 'air' },
      { x: -1, y: 0, z: 0, state: 'minecraft:cave_air' },
      { x: 1.5, y: 0, z: 0, state: 'air' },
    ])
    expect(u).toMatchObject({ changed: 0, outside: 3 })
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

/** 呼ばれた口を記録する偽の絵 (deepslate の StructureRenderer の代わり) */
class FakeRenderer implements RedrawTarget {
  readonly calls: string[] = []
  structure: StructureProvider
  constructor(structure: StructureProvider) {
    this.structure = structure
  }
  setStructure(structure: StructureProvider): void {
    this.structure = structure
    this.calls.push(`setStructure ${structure.getSize().join('x')}`)
  }
  updateStructureBuffers(chunks?: BlockPos[]): void {
    this.calls.push(chunks === undefined ? 'all' : `chunks ${chunks.map(c => c.join(',')).sort().join(' ')}`)
  }
}

/** IsometricView の代わりに、頼まれた事を記録する */
function recorder() {
  const log: string[] = []
  const hooks: ViewHooks = {
    names: names => log.push(`names ${names.join(',')}`),
    resized: size => log.push(`resized ${size.join('x')}`),
    draw: () => log.push('draw'),
  }
  return { log, hooks }
}

describe('盤面の変化を絵へ当てる配線 (BoardView。IsometricView が使う)', () => {
  // 48 x 4 x 48: 3 x 1 x 3 チャンク (半分は 4.5)
  const b = bounds([0, 0, 0], [47, 3, 47])
  const initial = { '0,0,0': 'stone', '20,1,20': LEVER_OFF }

  it('読み込み中は絵に触らず、準備ができた時 (attach) に、その時の構造から絵を作る', () => {
    const { log, hooks } = recorder()
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), ['stone', 'lever'], hooks)
    expect(view.renderer).toBeNull()
    expect(view.applyChanges([{ x: 20, y: 1, z: 20, state: LEVER_ON }]).step).toBe('loading')
    // 読み込み中に範囲が変わっても、絵が無いので寄り直しは頼まない (attach の後に IsometricView が合わせる)
    const bigger = bounds([0, 0, 0], [63, 3, 47])
    expect(view.setBoard(boardOf(bigger, { '0,0,0': 'stone', '20,1,20': LEVER_ON, '60,0,0': 'stone' })).step).toBe('loading')
    expect(log).toEqual([])

    let given: StructureProvider | null = null
    const r = view.attach(structure => (given = structure, new FakeRenderer(structure)))
    expect(view.renderer).toBe(r)
    expect(given).toBe(view.model.structure)
    expectSameStructure(r.structure, new BoardModel(boardOf(bigger, { '0,0,0': 'stone', '20,1,20': LEVER_ON, '60,0,0': 'stone' })).structure)
    expect(r.calls).toEqual([])
  })

  it('準備ができた後: 数マスならチャンクだけ、多ければ全部、範囲が変われば構造の差し替えと寄り直し。どれも描く予約をする', () => {
    const { log, hooks } = recorder()
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), [], hooks)
    const r = view.attach(structure => new FakeRenderer(structure))

    expect(view.applyChanges([{ x: 20, y: 1, z: 20, state: LEVER_ON }]).step).toBe('chunks')
    expect(view.applyChanges([{ x: 16, y: 0, z: 5, state: 'stone' }]).step).toBe('chunks')   // 境の隣も
    const many = [0, 1, 2].flatMap(cx => [0, 1, 2].map(cz => ({ x: cx * 16 + 5, y: 2, z: cz * 16 + 5, state: 'stone' })))
    expect(view.applyChanges(many).step).toBe('all')
    expect(view.applyChanges(many).step).toBe('none')                                    // 同じ物は何もしない
    const resized = view.setBoard(boardOf(bounds([0, 0, 0], [15, 3, 15]), { '0,0,0': 'stone' }))
    expect(resized).toEqual({ step: 'resized', result: { changed: 11, outside: 0 } })
    expect(r.structure).toBe(view.model.structure)

    expect(r.calls).toEqual(['chunks 1,0,1', 'chunks 0,0,0 1,0,0', 'all', 'setStructure 16x4x16'])
    expect(log).toEqual(['draw', 'draw', 'draw', 'resized 16x4x16', 'draw'])
  })

  it('知らない名前が出たら、絵を外して資源の作り直しを頼む。作り直す間の変化も、次の attach の構造に入る', () => {
    const { log, hooks } = recorder()
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), ['stone'], hooks)
    expect(view.resourceNames).toEqual(['lever', 'stone'])
    const r = view.attach(structure => new FakeRenderer(structure))

    const u = view.applyChanges([{ x: 3, y: 0, z: 3, state: LAMP(true) }])
    expect(u).toEqual({ step: 'names', result: { changed: 1, outside: 0 } })
    expect(view.renderer).toBeNull()
    expect(log).toEqual(['names lever,redstone_lamp,stone'])
    expect(r.calls).toEqual([])                                  // 外した絵には当てない
    expect(view.applyChanges([{ x: 4, y: 0, z: 3, state: LAMP(false) }]).step).toBe('loading')

    const r2 = view.attach(structure => new FakeRenderer(structure))
    expect(r2.structure.getBlock([4, 0, 3])?.state.toString()).toBe(`minecraft:${LAMP(false)}`)
    expect(r2.structure.getBlock([3, 0, 3])?.state.toString()).toBe(`minecraft:${LAMP(true)}`)
    // 名前は増えるだけ。preloadNames が知らない名前を足した時も作り直す
    expect(view.setPreload(['stone'])).toBe(false)
    expect(view.setPreload(['minecraft:glass[]', 'stone'])).toBe(true)
    expect(log.at(-1)).toBe('names glass,lever,redstone_lamp,stone')
  })

  it('preload に入っている名前が盤面に初めて出た時は、資源を作り直さず、チャンクだけ作り直す (絵は外さない)', () => {
    // アプリが状態表の名前を preloadNames に渡す使い方では、これが普通の道になる
    const { log, hooks } = recorder()
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), ['stone', 'lever', 'minecraft:redstone_lamp[lit=false]'], hooks)
    expect(view.resourceNames).toEqual(['lever', 'redstone_lamp', 'stone'])
    const r = view.attach(structure => new FakeRenderer(structure))
    expect(view.model.names.has('redstone_lamp')).toBe(false)   // 盤面にはまだ無い

    expect(view.applyChanges([{ x: 3, y: 0, z: 3, state: LAMP(true) }])).toEqual({ step: 'chunks', result: { changed: 1, outside: 0 } })
    expect(view.model.names.has('redstone_lamp')).toBe(true)    // 盤面に初めて出た (BoardModel は新しい名前として返した)
    expect(view.renderer).toBe(r)
    expect(r.calls).toEqual(['chunks 0,0,0'])
    expect(log).toEqual(['draw'])                               // 資源の作り直しは頼まない
    expect(view.resourceNames).toEqual(['lever', 'redstone_lamp', 'stone'])
    // setBoard の道も同じ
    expect(view.setBoard(boardOf(b, { ...initial, '40,2,40': LAMP(false) })).step).toBe('chunks')
    expect(log).toEqual(['draw', 'draw'])
  })

  it('絵が無い間 (最初の読み込み中・資源の作り直し中) に新しい名前が続けて出ても、そのたびに作り直しを頼み、資源に入る', () => {
    const { log, hooks } = recorder()
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), [], hooks)
    expect(view.applyChanges([{ x: 3, y: 0, z: 3, state: LAMP(true) }]).step).toBe('names')
    expect(view.applyChanges([{ x: 4, y: 0, z: 3, state: 'glass' }]).step).toBe('names')   // 作り直しの間にもう 1 つ
    expect(view.applyChanges([{ x: 5, y: 0, z: 3, state: LAMP(false) }]).step).toBe('loading')
    expect(log).toEqual(['names lever,redstone_lamp,stone', 'names glass,lever,redstone_lamp,stone'])
    expect(view.resourceNames).toEqual(['glass', 'lever', 'redstone_lamp', 'stone'])
    // IsometricView は最後に頼まれた名前で資源を作り、その時の構造で attach する
    const r = view.attach(structure => new FakeRenderer(structure))
    expect(r.structure.getBlock([4, 0, 3])?.state.toString()).toBe('minecraft:glass')
    expect(r.structure.getBlock([5, 0, 3])?.state.toString()).toBe(`minecraft:${LAMP(false)}`)
  })

  it('detach は、渡した絵が今の絵の時だけ外す (古い初期化の片付けが、新しい絵を外さない)', () => {
    const view = new BoardView<FakeRenderer>(boardOf(b, initial), [], recorder().hooks)
    const old = view.attach(structure => new FakeRenderer(structure))
    const now = view.attach(structure => new FakeRenderer(structure))
    view.detach(old)
    expect(view.renderer).toBe(now)
    view.detach(now)
    expect(view.renderer).toBeNull()
  })

  it('作った時の盤面の結果 (onBoardApplied で知らせる数) を持つ', () => {
    const m = new BoardModel(boardOf(bounds([0, 0, 0], [1, 0, 0]), { '0,0,0': 'stone', '5,0,0': 'stone', '1,0,0': 'air' }))
    expect(m.initial).toEqual({ changed: 1, outside: 1 })
  })
})

describe('WebGL の文脈を手放す (IsometricView の releaseWhenDetached)', () => {
  function fakeCanvas(isConnected: boolean, hasGl = true) {
    let lost = 0
    const ext = { loseContext: () => { lost++ } }
    const gl = { getExtension: (name: string) => (name === 'WEBGL_lose_context' ? ext : null) }
    const canvas = {
      isConnected,
      getContext: (id: 'webgl') => (id === 'webgl' && hasGl ? gl as unknown as WebGLRenderingContext : null),
    }
    return { canvas, lost: () => lost }
  }

  /** DOM の変化の見張りの代役。`fire` で変化を知らせる。付いているかを `watching` で見る */
  function fakeWatch() {
    let listener: (() => void) | null = null
    const watch: WatchDom = onChange => {
      listener = onChange
      return () => { listener = null }
    }
    return { watch, fire: () => listener?.(), watching: () => listener !== null }
  }

  it('もう DOM から外れていたら、すぐ手放す (見張らない)。WebGL が無ければ何もしない', () => {
    const c = fakeCanvas(false)
    const w = fakeWatch()
    releaseWhenDetached(c.canvas, w.watch)
    expect(c.lost()).toBe(1)
    expect(w.watching()).toBe(false)
    expect(() => releaseWhenDetached(fakeCanvas(false, false).canvas, w.watch)).not.toThrow()
  })

  it('Activity で隠した (DOM に残る) 間は手放さず、隠したまま外されたら手放す', () => {
    const c = fakeCanvas(true)
    const w = fakeWatch()
    releaseWhenDetached(c.canvas, w.watch)
    expect(c.lost()).toBe(0)
    expect(w.watching()).toBe(true)
    w.fire()                         // ほかの所が変わっただけ
    expect(c.lost()).toBe(0)
    c.canvas.isConnected = false     // 隠したまま外された
    w.fire()
    expect(c.lost()).toBe(1)
    expect(w.watching()).toBe(false)
    w.fire()
    expect(c.lost()).toBe(1)
  })

  it('外れる前に戻した (effect がやり直した) ら、見張りを止め、後で外れても手放さない (次の片付けが見る)', () => {
    const c = fakeCanvas(true)
    const w = fakeWatch()
    const stop = releaseWhenDetached(c.canvas, w.watch)
    stop()
    expect(w.watching()).toBe(false)
    c.canvas.isConnected = false
    w.fire()
    expect(c.lost()).toBe(0)
    expect(() => stop()).not.toThrow()
  })

  it('見張りを付ける間に外れても取りこぼさない (見張りの口が呼び返しをすぐ呼ぶ時も)', () => {
    const c = fakeCanvas(true)
    let stopped = 0
    const eager: WatchDom = onChange => {
      c.canvas.isConnected = false
      onChange()
      return () => { stopped++ }
    }
    releaseWhenDetached(c.canvas, eager)
    expect(c.lost()).toBe(1)
    expect(stopped).toBe(1)
  })
})

// ── 依存と書き出しの見張り ────────────────────────────────────────────────

const VIEW_DIR = fileURLToPath(new URL('..', import.meta.url))   // packages/view-3d/
const REPO_DIR = resolve(VIEW_DIR, '../..')

/** 見張る TS のファイル (.ts・.tsx・.mts・.cts。.d.ts も含む) */
const SOURCE_FILE = /\.[cm]?tsx?$/
/** 試験のファイル */
const TEST_FILE = /\.test\.[cm]?tsx?$/

/** 置き場 (`root` からの `dir`) の下の、見張る TS のファイル (`dir` からの道筋)。`root` は試験で一時のフォルダを渡す */
function sourcesOf(dir: string, root = VIEW_DIR): string[] {
  return (readdirSync(join(root, dir), { recursive: true }) as string[])
    .filter(f => SOURCE_FILE.test(f))
    .map(f => join(dir, f))
}

interface Specifier {
  readonly spec: string
  /**
   * `module`: import などの指定子・`types`: `/// <reference types=… />`・`path`: `/// <reference path=… />`・
   * `lib`: `/// <reference lib=… />`
   */
  readonly kind: 'module' | 'types' | 'path' | 'lib'
  readonly typeOnly: boolean
  readonly line: number
}

/**
 * ファイルの中の、モジュールを指す所を全部集める (構文木で見るので、引用符の種類・改行・注を問わない)。
 * import・export … from・import type・`import(…)`・`require(…)`・型の中の `import('…')`。指定子が文字列でなければ `<式>`。
 * 三重スラッシュの指示 (`types=`・`path=`・`lib=`) も集める。設定の `types: []` と `lib` は、ファイルの中の指示を止めない (#14)
 */
function specifiersOf(file: string, text: string): Specifier[] {
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const out: Specifier[] = []
  const at = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1
  const literal = (n: ts.Node | undefined) => (n && ts.isStringLiteralLike(n) ? n.text : '<式>')
  const visit = (n: ts.Node): void => {
    if (ts.isImportDeclaration(n)) {
      const c = n.importClause
      const named = c?.namedBindings && ts.isNamedImports(c.namedBindings) ? c.namedBindings.elements : null
      const typeOnly = !!c && (c.isTypeOnly || (!c.name && !!named && named.length > 0 && named.every(e => e.isTypeOnly)))
      out.push({ spec: literal(n.moduleSpecifier), kind: 'module', typeOnly, line: at(n) })
    } else if (ts.isExportDeclaration(n) && n.moduleSpecifier) {
      const named = n.exportClause && ts.isNamedExports(n.exportClause) ? n.exportClause.elements : null
      const typeOnly = n.isTypeOnly || (!!named && named.length > 0 && named.every(e => e.isTypeOnly))
      out.push({ spec: literal(n.moduleSpecifier), kind: 'module', typeOnly, line: at(n) })
    } else if (ts.isImportEqualsDeclaration(n) && ts.isExternalModuleReference(n.moduleReference)) {
      out.push({ spec: literal(n.moduleReference.expression), kind: 'module', typeOnly: n.isTypeOnly, line: at(n) })
    } else if (ts.isCallExpression(n)
      && (n.expression.kind === ts.SyntaxKind.ImportKeyword || (ts.isIdentifier(n.expression) && n.expression.text === 'require'))) {
      out.push({ spec: literal(n.arguments[0]), kind: 'module', typeOnly: false, line: at(n) })
    } else if (ts.isImportTypeNode(n) && ts.isLiteralTypeNode(n.argument)) {
      out.push({ spec: literal(n.argument.literal), kind: 'module', typeOnly: true, line: at(n) })
    }
    ts.forEachChild(n, visit)
  }
  visit(sf)
  const lineOf = (pos: number) => sf.getLineAndCharacterOfPosition(pos).line + 1
  for (const d of sf.typeReferenceDirectives) out.push({ spec: d.fileName, kind: 'types', typeOnly: true, line: lineOf(d.pos) })
  for (const d of sf.referencedFiles) out.push({ spec: d.fileName, kind: 'path', typeOnly: true, line: lineOf(d.pos) })
  for (const d of sf.libReferenceDirectives) out.push({ spec: d.fileName, kind: 'lib', typeOnly: true, line: lineOf(d.pos) })
  return out
}

/** パッケージの名前 (`@scope/name` か、最初の区切りまで) */
function packageOf(spec: string): string {
  const parts = spec.split('/')
  return spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0]
}

/** src の (試験でない) ファイルが使ってよいパッケージ */
const SRC_PACKAGES = new Set(['react', 'deepslate', 'gl-matrix'])
/** 試験が使ってよい物 (src の物に足す) */
const TEST_PACKAGES = new Set(['vitest', 'typescript'])
/** 型だけ依ってよいパッケージ (docs/architecture.md §1: board → view-3d) */
const TYPE_ONLY_PACKAGES = new Set(['@redstone-engine/board'])

/** 依存の決まりに外れた所 (file は packages/view-3d/ からの道筋) */
function dependencyHits(file: string, text: string): string[] {
  const isTest = TEST_FILE.test(file) || file.startsWith(`test${sep}`) || file.startsWith('test/')
  const hits: string[] = []
  const outside = (spec: string) => {
    const target = resolve(VIEW_DIR, dirname(file), spec)
    return relative(VIEW_DIR, target).startsWith('..') || resolve(target) === resolve(VIEW_DIR, '..')
  }
  for (const { spec, kind, typeOnly, line } of specifiersOf(file, text)) {
    const where = `${file}:${line} ${kind === 'module' ? spec : `/// ${kind}=${spec}`}`
    if (kind === 'path') {
      // path= は、先頭が . でなくてもファイルからの相対パスとして読まれる
      if (outside(spec)) hits.push(`${where} (view-3d の外を指す path=)`)
    } else if (kind === 'lib') {
      // lib に Node の型は無いが、ブラウザの設定の lib (ES2023・DOM・DOM.Iterable) を広げてしまう
      if (!isTest) hits.push(`${where} (src に lib= を書かない。ブラウザの設定の lib を広げる)`)
    } else if (kind === 'types') {
      // src では全部外れ (ブラウザの型検査の types: [] をすり抜けて @types/node などを読み込む)
      if (!isTest) hits.push(`${where} (src に types= を書かない。型の束を読み込む)`)
      else if (spec !== 'node' && !SRC_PACKAGES.has(packageOf(spec)) && !TEST_PACKAGES.has(packageOf(spec))) {
        hits.push(`${where} (許していない types=)`)
      }
    } else if (spec === '<式>') {
      hits.push(`${where} (指定子が文字列でない。確かめられない)`)
    } else if (spec.startsWith('.')) {
      if (outside(spec)) hits.push(`${where} (view-3d の外を指す相対パス)`)
    } else if (spec.startsWith('node:')) {
      if (!isTest) hits.push(`${where} (src に Node の物を入れない)`)
    } else {
      const pkg = packageOf(spec)
      if (TYPE_ONLY_PACKAGES.has(pkg)) {
        if (!typeOnly) hits.push(`${where} (board は型だけ)`)
      } else if (!SRC_PACKAGES.has(pkg) && !(isTest && TEST_PACKAGES.has(pkg))) {
        hits.push(`${where} (許していないパッケージ。engine-js・@redstone/sim には依らない)`)
      }
    }
  }
  return hits
}

/** index.ts が書き出す名前 (`export *` は名前が決まらないので、それ自体を外れとして返す) */
function exportedNamesOf(text: string): { names: string[]; star: number } {
  const sf = ts.createSourceFile('index.ts', text, ts.ScriptTarget.Latest, true)
  const names: string[] = []
  let star = 0
  for (const st of sf.statements) {
    if (ts.isExportDeclaration(st)) {
      if (!st.exportClause || ts.isNamespaceExport(st.exportClause)) star++
      else for (const e of st.exportClause.elements) names.push(e.name.text)
    } else if (ts.isExportAssignment(st)) {
      names.push(st.isExportEquals ? 'export=' : 'default')        // export default <式> と export = <式>
    } else if (ts.canHaveModifiers(st) && ts.getModifiers(st)?.some(m => m.kind === ts.SyntaxKind.ExportKeyword)) {
      const isDefault = ts.getModifiers(st)?.some(m => m.kind === ts.SyntaxKind.DefaultKeyword)
      if (isDefault) names.push('default')
      else if (ts.isVariableStatement(st)) for (const d of st.declarationList.declarations) names.push(d.name.getText(sf))
      else names.push((st as ts.DeclarationStatement).name?.getText(sf) ?? '<名前なし>')
    }
  }
  return { names: names.sort(), star }
}

/**
 * 窓口として書き出す名前 (足す・外す時はここも直す)。
 * 盤面の型 (Board・BoardBounds・CellChange・PickRole・PickRoleFn・Pos3) は board が書き出す (#14)
 */
const PUBLIC_NAMES = [
  'ApplyResult', 'AssetState', 'CameraInput', 'CameraState',
  'IsometricView', 'IsometricViewHandle', 'IsometricViewProps', 'MCMETA_BASE', 'PICK_PASS_THROUGH',
  'VIEWER_FAR', 'VIEWER_FOV_DEG', 'VIEWER_NEAR',
  'cellKey', 'fitDistance', 'maxZoomOut', 'normalizeState', 'pickRoleFrom',
].sort()

/**
 * 型検査器で、モジュールが書き出す物の型をたどり、deepslate で宣言された型が出てこないかを見る。
 * props の中・関数の引数と戻り値・型の引数まで降りる。React と lib の型の中へは降りない (型の引数は見る)
 */
function deepslateLeaksOf(program: ts.Program, file: string): string[] {
  const checker = program.getTypeChecker()
  const sf = program.getSourceFile(file)
  if (!sf) throw new Error(`読めない: ${file}`)
  const mod = checker.getSymbolAtLocation(sf)
  if (!mod) throw new Error(`モジュールでない: ${file}`)
  const inside = (s: ts.Symbol | undefined, re: RegExp) => !!s?.declarations?.some(d => re.test(d.getSourceFile().fileName))
  const DEEPSLATE = /[\\/]node_modules[\\/]deepslate[\\/]/
  const leaks = new Set<string>()
  // 書き出す物ごとに作り直す (前の物で見た型を飛ばして、後の物の漏れを見落とさない)
  let seen = new Set<ts.Type>()
  const walk = (t: ts.Type, path: string): void => {
    if (seen.has(t)) return
    seen.add(t)
    const sym = t.aliasSymbol ?? t.getSymbol()
    if (inside(t.aliasSymbol, DEEPSLATE) || inside(t.getSymbol(), DEEPSLATE)) {
      leaks.add(`${path}: ${checker.typeToString(t)}`)
      return
    }
    for (const a of t.aliasTypeArguments ?? []) walk(a, `${path}<>`)
    if ((t.flags & ts.TypeFlags.Object) && ((t as ts.ObjectType).objectFlags & ts.ObjectFlags.Reference)) {
      for (const a of checker.getTypeArguments(t as ts.TypeReference)) walk(a, `${path}<>`)
    }
    if (t.isUnionOrIntersection()) for (const u of t.types) walk(u, path)
    if (t.flags & ts.TypeFlags.TypeParameter) {
      // 型の引数は制約だけを見る (制約の中身は制約の型をたどる時に見る)
      const c = checker.getBaseConstraintOfType(t)
      if (c && c !== t) walk(c, `${path} extends`)
      return
    }
    const decls = sym?.declarations ?? []
    if (decls.length > 0 && decls.every(d => /[\\/]node_modules[\\/]/.test(d.getSourceFile().fileName))) return
    for (const p of checker.getPropertiesOfType(t)) {
      const d = p.valueDeclaration ?? p.declarations?.[0]
      if (d) walk(checker.getTypeOfSymbolAtLocation(p, d), `${path}.${p.name}`)
    }
    for (const info of checker.getIndexInfosOfType(t)) {
      walk(info.keyType, `${path}[key]`)
      walk(info.type, `${path}[]`)
    }
    for (const sig of [...t.getCallSignatures(), ...t.getConstructSignatures()]) {
      for (const tp of sig.getTypeParameters() ?? []) walk(tp, `${path}<${tp.symbol.name}>`)
      for (const prm of sig.getParameters()) {
        const d = prm.valueDeclaration ?? prm.declarations?.[0]
        if (d) walk(checker.getTypeOfSymbolAtLocation(prm, d), `${path}(${prm.name})`)
      }
      walk(sig.getReturnType(), `${path}()`)
    }
  }
  for (const exported of checker.getExportsOfModule(mod)) {
    seen = new Set<ts.Type>()
    const s = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported
    if (inside(s, DEEPSLATE)) leaks.add(`${exported.name}: deepslate の物そのもの`)
    const d = s.valueDeclaration ?? s.declarations?.[0]
    if ((s.flags & ts.SymbolFlags.Value) && d) walk(checker.getTypeOfSymbolAtLocation(s, d), exported.name)
    if (s.flags & ts.SymbolFlags.Type) walk(checker.getDeclaredTypeOfSymbol(s), exported.name)
  }
  return [...leaks].sort()
}

describe('依存の向き (docs/architecture.md §1・§2)', () => {
  const files = [...sourcesOf('src'), ...sourcesOf('test')].map(f => ({ f, text: readFileSync(join(VIEW_DIR, f), 'utf-8') }))

  it('engine-js・@redstone/sim に依らない。board は型だけ。相対パスで view-3d の外へ出ない。src に Node の物を入れない', () => {
    expect(files.length).toBeGreaterThan(10)
    expect(files.flatMap(({ f, text }) => dependencyHits(f, text))).toEqual([])
  })

  it('見張りは、引用符の種類・相対パス・動的 import・export from・require・型の import を拾う', () => {
    // 見張りの試験の字面が、この試験ファイル自身を見張りに引っかけないよう、IMPORT を置き換えて作る
    const src = (t: string) => t.replace(/IMPORT/g, 'import')
    const hits = (t: string, file = join('src', 'x.ts')) => dependencyHits(file, src(t))
    expect(hits('IMPORT { Engine } from "@redstone-engine/engine-js"')).toHaveLength(1)
    expect(hits("IMPORT type { BlockState } from '@redstone/sim'")).toHaveLength(1)
    expect(hits('IMPORT { x } from "../../engine-js/src/index"')).toHaveLength(1)
    expect(hits("IMPORT { x } from '../src/world'", join('test', 'a.test.ts'))).toEqual([])
    expect(hits("IMPORT { x } from '../../board/src/types'", join('test', 'a.test.ts'))).toHaveLength(1)
    expect(hits("const m = await IMPORT('@redstone-engine/engine-js')")).toHaveLength(1)
    expect(hits('const m = await IMPORT(name)')).toHaveLength(1)
    expect(hits("export * from '@redstone/sim'")).toHaveLength(1)
    expect(hits("const m = require('@redstone-engine/engine-js')")).toHaveLength(1)
    expect(hits("let t: IMPORT('@redstone/sim').BlockState")).toHaveLength(1)
    expect(hits("IMPORT {\n  readFile,\n} from 'node:fs/promises'")).toHaveLength(1)
    expect(hits("IMPORT { readFile } from 'node:fs'", join('src', 'a.test.ts'))).toEqual([])
    // board は型だけ
    expect(hits("IMPORT type { Board } from '@redstone-engine/board'")).toEqual([])
    expect(hits("IMPORT { type Board } from '@redstone-engine/board'")).toEqual([])
    expect(hits("IMPORT { Board } from '@redstone-engine/board'")).toHaveLength(1)
    // 注の中の字面は拾わない
    expect(hits("// IMPORT { Engine } from '@redstone-engine/engine-js'\nconst a = 1")).toEqual([])
  })

  it('見張りは、三重スラッシュの指示 (types=・path=・lib=) と .mts・.cts も拾う (#14)', () => {
    const hits = (t: string, file = join('src', 'x.ts')) => dependencyHits(file, t.replace(/IMPORT/g, 'import'))
    const ref = (attr: string) => `/// <reference ${attr} />\nexport const a = 1`
    // types= は src では全部外れ (ブラウザの型検査の types: [] をすり抜けて @types/node などを読み込む)
    expect(hits(ref('types="node"'))).toHaveLength(1)
    expect(hits(ref("types='react'"))).toHaveLength(1)
    expect(hits(ref('types="node"'), join('test', 'a.test.ts'))).toEqual([])
    expect(hits(ref('types="vite/client"'), join('test', 'a.test.ts'))).toHaveLength(1)
    // path= は view-3d の外を指す物が外れ (先頭が . でなくても相対)
    expect(hits(ref('path="./types.ts"'))).toEqual([])
    expect(hits(ref('path="renderer/picking.ts"'))).toEqual([])
    expect(hits(ref('path="../../engine-js/src/node.ts"'))).toHaveLength(1)
    expect(hits(ref('path="sub/../../../engine-js/src/node.ts"'))).toHaveLength(1)   // 先頭が . でなくても外へ出る
    expect(hits(ref('path="../../board/src/types.ts"'), join('test', 'a.test.ts'))).toHaveLength(1)
    // lib= は src では外れ (ブラウザの設定の lib を広げる)。試験は通す
    expect(hits(ref('lib="esnext"'))).toHaveLength(1)
    expect(hits(ref('lib="dom"'))).toHaveLength(1)
    expect(hits(ref('lib="esnext"'), join('test', 'a.test.ts'))).toEqual([])
    // .mts・.cts も読み、試験のファイルの見分けも同じ
    for (const f of ['a.ts', 'a.tsx', 'a.mts', 'a.cts', 'a.d.ts', 'a.d.mts']) expect(SOURCE_FILE.test(f), f).toBe(true)
    for (const f of ['a.js', 'a.mjs', 'a.json', 'a.ts.bak']) expect(SOURCE_FILE.test(f), f).toBe(false)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('src', 'x.mts'))).toHaveLength(1)
    expect(hits("IMPORT { Engine } from '@redstone-engine/engine-js'", join('src', 'x.cts'))).toHaveLength(1)
    expect(hits("IMPORT { readFileSync } from 'node:fs'", join('src', 'x.test.mts'))).toEqual([])
  })

  it('見張るファイルの一覧は .ts・.tsx・.mts・.cts と型の宣言を拾い、.js は拾わない (一時のフォルダで確かめる)', () => {
    const root = mkdtempSync(join(tmpdir(), 'view-3d-guard-'))
    try {
      mkdirSync(join(root, 'src', 'sub'), { recursive: true })
      const files: Record<string, string> = {
        'a.ts': 'export const a = 1',
        'b.mts': "IMPORT { readFileSync } from 'node:fs'\nexport const b = readFileSync",
        'c.cts': 'export const c = 1',
        'd.js': "IMPORT { readFileSync } from 'node:fs'",
        'e.d.mts': 'export declare const e: number',
        'sub/f.tsx': 'export const f = 1',
      }
      for (const [name, text] of Object.entries(files)) writeFileSync(join(root, 'src', name), text.replace(/IMPORT/g, 'import'))
      const listed = sourcesOf('src', root)
      expect([...listed].sort()).toEqual(['a.ts', 'b.mts', 'c.cts', 'e.d.mts', join('sub', 'f.tsx')].map(f => join('src', f)).sort())
      // 一覧に入った .mts の node: の import を、依存の見張りが拾う (view-3d の src は .tsx を置いてよい)
      const hits = listed.flatMap(f => dependencyHits(f, readFileSync(join(root, f), 'utf-8')))
      expect(hits).toHaveLength(1)
      expect(hits[0].startsWith(`${join('src', 'b.mts')}:1 node:fs`), hits.join(' / ')).toBe(true)
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  it('index.ts は export * を使わず、書き出す名前は決めた一覧のとおり', () => {
    const index = files.find(x => x.f === join('src', 'index.ts'))?.text ?? ''
    const { names, star } = exportedNamesOf(index)
    expect(star).toBe(0)
    expect(names).toEqual(PUBLIC_NAMES)
    expect(exportedNamesOf("export * from './x'\nexport * as y from './y'").star).toBe(2)
    expect(exportedNamesOf('export default 42').names).toEqual(['default'])
    expect(exportedNamesOf('export default function f() {}').names).toEqual(['default'])
    expect(exportedNamesOf('export const a = 1, b = 2\nexport function c() {}').names).toEqual(['a', 'b', 'c'])
    expect(files.find(x => x.f === join('src', 'types.ts'))?.text).not.toMatch(/^import /m)
  })

  it('書き出しの型 (props・命令の口・関数の引数と戻り値) に deepslate の型が出ない (型検査器でたどる)', () => {
    const cfgPath = join(REPO_DIR, 'tsconfig.json')
    const cfg = ts.parseJsonConfigFileContent(ts.readConfigFile(cfgPath, ts.sys.readFile).config, ts.sys, REPO_DIR)
    const index = join(VIEW_DIR, 'src', 'index.ts')
    const inner = join(VIEW_DIR, 'src', 'board-to-structure.ts')
    const program = ts.createProgram([index, inner], cfg.options)
    expect(deepslateLeaksOf(program, index)).toEqual([])
    // 見張りが効くこと: 中の物 (new BoardModel().structure・Redraw の chunks) は deepslate の型を出している
    const inside = deepslateLeaksOf(program, inner)
    expect(inside).toContain('BoardModel().structure: StructureProvider')
    expect(inside).toContain('Redraw.chunks<>: BlockPos')
  }, 60_000)

  it('型の見張りは、索引署名の値・型の引数の制約 (関数・interface・型の別名) へも降りる', () => {
    // 盤面に置かない仮のファイル (view-3d の src の中にあるとして解く) を、手元の型検査器に渡す
    const probe = join(VIEW_DIR, 'src', '__probe__.ts')
    const text = [
      "import type { BlockState } from 'deepslate/core'",
      'export interface ByName { readonly [name: string]: BlockState }',
      'export function pick<T extends BlockState>(name: string): string { return name }',
      'export interface Box<T extends BlockState> { readonly label: string }',
      'export type Alias<T extends BlockState = BlockState> = { readonly label: string }',
      'export interface Clean { readonly [name: string]: string }',
    ].join('\n')
    const cfgPath = join(REPO_DIR, 'tsconfig.json')
    const cfg = ts.parseJsonConfigFileContent(ts.readConfigFile(cfgPath, ts.sys.readFile).config, ts.sys, REPO_DIR)
    const host = ts.createCompilerHost(cfg.options)
    const getSourceFile = host.getSourceFile.bind(host)
    host.getSourceFile = (name, lang, ...rest) =>
      (resolve(name) === probe ? ts.createSourceFile(name, text, lang) : getSourceFile(name, lang, ...rest))
    const fileExists = host.fileExists.bind(host)
    host.fileExists = name => resolve(name) === probe || fileExists(name)
    const program = ts.createProgram([probe], cfg.options, host)
    const leaks = deepslateLeaksOf(program, probe)
    for (const name of ['ByName', 'pick', 'Box', 'Alias']) {
      expect(leaks.some(l => l.startsWith(name)), `${name}: ${leaks.join(' / ')}`).toBe(true)
    }
    expect(leaks.some(l => l.startsWith('Clean'))).toBe(false)
  }, 60_000)
})
