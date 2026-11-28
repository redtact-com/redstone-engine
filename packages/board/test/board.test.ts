import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import {
  BOARD_BOUNDS, EDIT_Y, FLOOR_STATE, FLOOR_Y, PLACE_STATES, TOOLS,
  applyChanges, blockNamesOf, cellKey, cellsToLoad, createBoard, isAllOff, isEditable, isSameBlock,
  type Board, type CellChange,
} from '../src/index'

const LEVER_OFF = 'lever[face=floor,facing=north,powered=false]'
const LEVER_ON = 'lever[face=floor,facing=north,powered=true]'
const DUST_DOT = 'redstone_wire[east=none,north=none,power=0,south=none,west=none]'
const DUST_LINE = (power: number) => `redstone_wire[east=side,north=none,power=${power},south=none,west=side]`
const LAMP = (lit: boolean) => `redstone_lamp[lit=${lit}]`

/** 床の盤面に、y=1 のマスを足した物 */
function boardWith(cells: Record<string, string>): Board {
  const b = createBoard()
  return { bounds: b.bounds, blocks: new Map([...b.blocks, ...Object.entries(cells)]) }
}

const change = (x: number, y: number, z: number, state: string): CellChange => ({ x, y, z, state })

describe('範囲と床', () => {
  it('x・z は 0〜15、y は床 (0) と編集する段 (1)。凍っていて書き換えられない', () => {
    expect(BOARD_BOUNDS).toEqual({ x: [0, 15], y: [0, 1], z: [0, 15] })
    expect([FLOOR_Y, EDIT_Y]).toEqual([0, 1])
    expect(Object.isFrozen(BOARD_BOUNDS) && Object.isFrozen(BOARD_BOUNDS.x) && Object.isFrozen(BOARD_BOUNDS.y)
      && Object.isFrozen(BOARD_BOUNDS.z)).toBe(true)
  })

  it('床だけの盤面は、y=0 の 256 マスが全部石で、ほかのマスは無い', () => {
    const b = createBoard()
    expect(b.bounds).toBe(BOARD_BOUNDS)
    expect(b.blocks.size).toBe(256)
    for (let z = 0; z <= 15; z++) {
      for (let x = 0; x <= 15; x++) expect(b.blocks.get(cellKey(x, 0, z))).toBe(FLOOR_STATE)
    }
    expect(FLOOR_STATE).toBe('stone')
    expect([...b.blocks.keys()].every(k => k.split(',')[1] === '0')).toBe(true)
  })

  it('作るたびに別の表を返す (片方を当てても、もう片方は変わらない)', () => {
    const a = createBoard()
    const b = createBoard()
    expect(a.blocks).not.toBe(b.blocks)
    applyChanges(a, [change(0, 1, 0, LEVER_OFF)])
    expect(b.blocks.size).toBe(256)
  })

  it('原点は fixture と同じ: fixture の回路は盤面の範囲に収まり、床の段は石・素子の段は y=1', () => {
    for (const name of ['lever-wire-lamp', 'lamp-off-delay']) {
      const fx = fixture(name)
      expect(fx.region.from).toEqual([0, 0, 0])
      for (const { pos: [x, y, z], block } of fx.blocks) {
        expect(x >= 0 && x <= 15 && z >= 0 && z <= 15 && (y === 0 || y === 1), `${name} ${x},${y},${z}`).toBe(true)
        if (y === FLOOR_Y) expect(block).toBe(FLOOR_STATE)
        else expect(isEditable(x, y, z)).toBe(true)
      }
    }
  })
})

describe('鍵', () => {
  it('"x,y,z" の形 (描く側の鍵と同じ)', () => {
    expect(cellKey(1, 2, 3)).toBe('1,2,3')
    expect(cellKey(0, 1, 15)).toBe('0,1,15')
    expect(cellKey(-4, 0, -12)).toBe('-4,0,-12')
  })
})

describe('編集できるマス (断る所)', () => {
  it('範囲の中の y=1 だけ編集できる', () => {
    expect(isEditable(0, 1, 0)).toBe(true)
    expect(isEditable(15, 1, 15)).toBe(true)
    expect(isEditable(7, 1, 3)).toBe(true)
  })

  it('範囲の外と y≠1 (床と、その上下) は断る。整数でない座標も断る', () => {
    for (const [x, y, z] of [
      [16, 1, 0], [-1, 1, 0], [0, 1, 16], [0, 1, -1],     // 範囲の外
      [0, 0, 0], [15, 0, 15],                              // 床
      [0, 2, 0], [0, -1, 0],                               // 床の下・編集する段の上
      [0.5, 1, 0], [0, 1, Number.NaN], [Infinity, 1, 0],  // 整数でない
    ]) {
      expect(isEditable(x, y, z), `${x},${y},${z}`).toBe(false)
    }
  })
})

describe('変化の当て方 (applyChanges)', () => {
  it('範囲の中の変化を当てた新しい盤面を返し、渡した盤面は書き換えない', () => {
    const b = createBoard()
    const next = applyChanges(b, [change(0, 1, 0, LEVER_OFF), change(1, 1, 0, DUST_DOT)])
    expect(next).not.toBe(b)
    expect(next.bounds).toBe(b.bounds)
    expect(next.blocks.get('0,1,0')).toBe(LEVER_OFF)
    expect(next.blocks.get('1,1,0')).toBe(DUST_DOT)
    expect(next.blocks.size).toBe(258)
    expect(b.blocks.size).toBe(256)
    expect(b.blocks.has('0,1,0')).toBe(false)
  })

  it('air・cave_air・void_air (minecraft: 付きも) は、表から消す', () => {
    const b = boardWith({ '0,1,0': LEVER_OFF, '1,1,0': DUST_DOT, '2,1,0': LAMP(false), '3,1,0': LAMP(false) })
    const next = applyChanges(b, [
      change(0, 1, 0, 'air'), change(1, 1, 0, 'cave_air'), change(2, 1, 0, 'void_air'), change(3, 1, 0, 'minecraft:air'),
    ])
    expect(next.blocks.size).toBe(256)
    for (const k of ['0,1,0', '1,1,0', '2,1,0', '3,1,0']) expect(next.blocks.has(k)).toBe(false)
    // 渡した盤面は書き換えない (消す側も)
    expect(b.blocks.size).toBe(260)
    expect([b.blocks.get('0,1,0'), b.blocks.get('1,1,0'), b.blocks.get('2,1,0'), b.blocks.get('3,1,0')])
      .toEqual([LEVER_OFF, DUST_DOT, LAMP(false), LAMP(false)])
  })

  it('範囲の外と、座標が整数でない変化は当てない (それだけなら同じ盤面を返す)', () => {
    const b = createBoard()
    const outside = [
      change(16, 1, 0, LEVER_OFF), change(-1, 1, 0, LEVER_OFF), change(0, 2, 0, LEVER_OFF), change(0, -1, 0, 'stone'),
      change(0, 1, 16, LEVER_OFF), change(0.5, 1, 0, LEVER_OFF), change(0, 1, Number.NaN, LEVER_OFF),
    ]
    expect(applyChanges(b, outside)).toBe(b)
    const mixed = applyChanges(b, [...outside, change(4, 1, 4, LAMP(true))])
    expect([...mixed.blocks.keys()].filter(k => !b.blocks.has(k))).toEqual(['4,1,4'])
  })

  it('変わらなければ同じ盤面 (同じ参照) を返す: 空の列・同じ状態・無いマスを消す・A → B → A', () => {
    const b = boardWith({ '0,1,0': LEVER_OFF })
    expect(applyChanges(b, [])).toBe(b)
    expect(applyChanges(b, [change(0, 1, 0, LEVER_OFF), change(0, 0, 0, 'stone')])).toBe(b)
    expect(applyChanges(b, [change(5, 1, 5, 'air')])).toBe(b)
    expect(applyChanges(b, [change(0, 1, 0, LEVER_ON), change(0, 1, 0, LEVER_OFF)])).toBe(b)
    expect(applyChanges(b, [change(5, 1, 5, DUST_DOT), change(5, 1, 5, 'air')])).toBe(b)
  })

  it('同じマスが何度出ても、最後の状態だけを当てる', () => {
    const b = createBoard()
    const next = applyChanges(b, [change(1, 1, 0, DUST_DOT), change(1, 1, 0, DUST_LINE(15)), change(1, 1, 0, DUST_LINE(14))])
    expect(next.blocks.get('1,1,0')).toBe(DUST_LINE(14))
  })

  it('床の変化も当てる (盤面はエンジンの世界の写し)。渡した盤面の床は残る', () => {
    const b = createBoard()
    const next = applyChanges(b, [change(3, 0, 3, 'air'), change(4, 0, 4, 'glass')])
    expect(next.blocks.has('3,0,3')).toBe(false)
    expect(next.blocks.get('4,0,4')).toBe('glass')
    expect(b.blocks.size).toBe(256)
    expect([b.blocks.get('3,0,3'), b.blocks.get('4,0,4')]).toEqual([FLOOR_STATE, FLOOR_STATE])
  })

  it('範囲は盤面の bounds で決める (BOARD_BOUNDS ではない)', () => {
    const narrow: Board = { bounds: { x: [0, 3], y: [0, 1], z: [0, 0] }, blocks: new Map([['0,0,0', 'stone']]) }
    // BOARD_BOUNDS の中でも、狭い盤面の外なら当てない
    expect(applyChanges(narrow, [change(5, 1, 0, LEVER_OFF), change(0, 1, 1, LEVER_OFF)])).toBe(narrow)
    expect(applyChanges(narrow, [change(3, 1, 0, LEVER_OFF)]).blocks.get('3,1,0')).toBe(LEVER_OFF)
    // BOARD_BOUNDS の外でも、広い盤面の中なら当てる
    const wide: Board = { bounds: { x: [-8, 31], y: [0, 3], z: [0, 15] }, blocks: new Map() }
    expect(applyChanges(wide, [change(-8, 3, 0, LAMP(false)), change(31, 2, 15, DUST_DOT)]).blocks.size).toBe(2)
  })

  it('列は配列のほか、繰り返せる物なら何でもよい (1 回だけ読む)', () => {
    let reads = 0
    function* gen(): Generator<CellChange> {
      reads++
      yield change(0, 1, 0, LEVER_ON)
    }
    expect(applyChanges(createBoard(), gen()).blocks.get('0,1,0')).toBe(LEVER_ON)
    expect(reads).toBe(1)
  })
})

describe('エンジンへ読み込む列 (cellsToLoad)', () => {
  it('床だけの盤面は、床の 256 マスを z・x の順で並べる', () => {
    const list = cellsToLoad(createBoard())
    expect(list).toHaveLength(256)
    expect(list.slice(0, 3)).toEqual([change(0, 0, 0, 'stone'), change(1, 0, 0, 'stone'), change(2, 0, 0, 'stone')])
    expect(list[16]).toEqual(change(0, 0, 1, 'stone'))
    expect(list[255]).toEqual(change(15, 0, 15, 'stone'))
  })

  it('並びは y・z・x の小さい順で、表に入れた順によらない。fixture の blocks と同じ並びになる', () => {
    const fx = fixture('lever-wire-lamp')
    const expected = fx.blocks.map(({ pos: [x, y, z], block }) => change(x, y, z, block))
    const forward: Board = { bounds: BOARD_BOUNDS, blocks: new Map(expected.map(c => [cellKey(c.x, c.y, c.z), c.state])) }
    const reversed: Board = { bounds: BOARD_BOUNDS, blocks: new Map([...forward.blocks].reverse()) }
    expect(cellsToLoad(forward)).toEqual(expected)
    expect(cellsToLoad(reversed)).toEqual(expected)
  })

  it('床が先 (y=0 の全部が y=1 のどれよりも前)。何も無いマスは入れない', () => {
    const b = boardWith({ '15,1,0': LAMP(false), '0,1,15': LEVER_OFF, '3,1,3': 'air', '4,1,4': 'minecraft:cave_air' })
    const list = cellsToLoad(b)
    expect(list).toHaveLength(258)
    expect(list.slice(0, 256).every(c => c.y === 0)).toBe(true)
    expect(list.slice(256)).toEqual([change(15, 1, 0, LAMP(false)), change(0, 1, 15, LEVER_OFF)])
  })

  it('鍵が "x,y,z" の形でないマスと、範囲の外のマスがあれば断る (黙って落とさない)', () => {
    for (const key of ['1,1', '1,1,1,1', 'a,1,1', '01,1,1', '+1,1,1', ' 1,1,1', '1.5,1,1', '-0,1,1', '1e1,1,1']) {
      expect(() => cellsToLoad(boardWith({ [key]: LEVER_OFF })), key).toThrow(RangeError)
    }
    for (const key of ['16,1,0', '0,2,0', '0,-1,0', '-1,0,0']) {
      expect(() => cellsToLoad(boardWith({ [key]: LEVER_OFF })), key).toThrow(RangeError)
    }
    // 何も無いマスは、範囲の外でも鍵が崩れていても読み込まないので断らない
    expect(cellsToLoad(boardWith({ '99,9,9': 'air' }))).toHaveLength(256)
    expect(cellsToLoad(boardWith({ 'a,b,c': 'air', '01,1,1': 'cave_air', '1,1': 'minecraft:void_air' }))).toHaveLength(256)
  })

  it('範囲は盤面の bounds で決める (BOARD_BOUNDS ではない)', () => {
    const narrow: Board = { bounds: { x: [0, 3], y: [0, 1], z: [0, 0] }, blocks: new Map([['0,0,0', 'stone'], ['5,1,0', LEVER_OFF]]) }
    expect(() => cellsToLoad(narrow)).toThrow(RangeError)
    const wide: Board = { bounds: { x: [-8, 31], y: [0, 3], z: [0, 15] }, blocks: new Map([['31,2,15', DUST_DOT], ['-8,3,0', LAMP(false)]]) }
    expect(cellsToLoad(wide)).toEqual([change(31, 2, 15, DUST_DOT), change(-8, 3, 0, LAMP(false))])
  })
})

describe('道具ごとの置きたい状態', () => {
  it('道具はレバー・ダスト・ランプ・消しゴムの順で、置く道具は状態の文字列を持つ', () => {
    expect(TOOLS).toEqual(['lever', 'dust', 'lamp', 'eraser'])
    expect(PLACE_STATES).toEqual({ lever: LEVER_OFF, dust: DUST_DOT, lamp: LAMP(false) })
    expect(Object.isFrozen(TOOLS) && Object.isFrozen(PLACE_STATES)).toBe(true)
  })

  it('レバーの文字列は fixture と同じ', () => {
    for (const name of ['lever-wire-lamp', 'lamp-off-delay']) {
      expect(fixture(name).blocks.filter(b => b.block.startsWith('lever')).map(b => b.block)).toEqual([PLACE_STATES.lever])
    }
  })

  it('どれもプロパティを名前順で全部書き、minecraft: を付けない (StateTable.name の形)', () => {
    for (const s of [...Object.values(PLACE_STATES), FLOOR_STATE]) {
      expect(s.startsWith('minecraft:')).toBe(false)
      const open = s.indexOf('[')
      if (open < 0) continue
      const keys = s.slice(open + 1, -1).split(',').map(kv => kv.split('=')[0])
      expect(keys, s).toEqual([...keys].sort())
    }
  })

  it('engine-js の状態表 (rse-cli の生成物) に、その名前のまま入っている。ダストとランプは既定の状態', () => {
    // generated/ は `npm run build:engine` (npm test の前の pretest) が作る。import せずに JSON として読むだけ
    const url = new URL('../../engine-js/generated/states.json', import.meta.url)
    let text: string
    try {
      text = readFileSync(url, 'utf-8')
    } catch {
      throw new Error('packages/engine-js/generated/states.json が無い。先に npm run build:engine (npm test なら pretest が作る)')
    }
    const table = JSON.parse(text) as {
      readonly blocks: readonly { readonly name: string; readonly defaultState: number }[]
      readonly states: { readonly name: readonly string[] }
    }
    const names = new Set(table.states.name)
    for (const s of [...Object.values(PLACE_STATES), FLOOR_STATE, 'air']) expect(names.has(s), s).toBe(true)
    const defaultOf = (block: string) => {
      const b = table.blocks.find(x => x.name === block)
      if (!b) throw new Error(`状態表に ${block} が無い`)
      return table.states.name[b.defaultState]
    }
    expect(defaultOf('redstone_wire')).toBe(PLACE_STATES.dust)
    expect(defaultOf('redstone_lamp')).toBe(PLACE_STATES.lamp)
    // レバーの既定は壁置き (V09 §3.1)。床置きを書いているので既定とは違う
    expect(defaultOf('lever')).not.toBe(PLACE_STATES.lever)
  })
})

describe('同じブロックか (isSameBlock)', () => {
  it('ブロックの名前で比べる: 形と power の違うダストどうしは同じ (なぞるたびに置き直さない)', () => {
    expect(isSameBlock(DUST_LINE(15), PLACE_STATES.dust)).toBe(true)
    expect(isSameBlock('redstone_wire[east=up,north=side,power=3,south=none,west=none]', DUST_DOT)).toBe(true)
    expect(isSameBlock(LEVER_ON, PLACE_STATES.lever)).toBe(true)
    expect(isSameBlock(LAMP(true), PLACE_STATES.lamp)).toBe(true)
    expect(isSameBlock('minecraft:redstone_lamp[lit=true]', 'redstone_lamp')).toBe(true)
  })

  it('名前が違えば違う', () => {
    expect(isSameBlock(DUST_DOT, PLACE_STATES.lamp)).toBe(false)
    expect(isSameBlock(LEVER_OFF, PLACE_STATES.dust)).toBe(false)
    expect(isSameBlock('stone', PLACE_STATES.lamp)).toBe(false)
  })

  it('何も無いマス (undefined・air・cave_air・void_air) どうしは同じで、ブロックとは違う', () => {
    expect(isSameBlock(undefined, undefined)).toBe(true)
    expect(isSameBlock(undefined, 'air')).toBe(true)
    expect(isSameBlock('minecraft:cave_air', 'void_air')).toBe(true)
    expect(isSameBlock(undefined, PLACE_STATES.dust)).toBe(false)
    expect(isSameBlock('air', PLACE_STATES.lever)).toBe(false)
  })
})

describe('見る (名前の一覧・全部切れか)', () => {
  it('盤面に出るブロックの名前を、重ねず名前順に返す (プロパティと minecraft: を外し、air は入れない)', () => {
    expect(blockNamesOf(createBoard())).toEqual(['stone'])
    const b = boardWith({ '0,1,0': LEVER_ON, '1,1,0': DUST_DOT, '2,1,0': DUST_LINE(15), '3,1,0': 'minecraft:redstone_lamp[lit=true]', '4,1,0': 'air' })
    expect(blockNamesOf(b)).toEqual(['lever', 'redstone_lamp', 'redstone_wire', 'stone'])
  })

  it('全部切れ: レバーは切・ダストは 0・ランプは消灯なら true。床だけの盤面も true', () => {
    expect(isAllOff(createBoard())).toBe(true)
    expect(isAllOff(boardWith({ '0,1,0': LEVER_OFF, '1,1,0': DUST_LINE(0), '2,1,0': DUST_DOT, '3,1,0': LAMP(false) }))).toBe(true)
  })

  it('1 つでも点いていれば false (レバーの powered=true・ダストの power が 0 でない・ランプの lit=true)', () => {
    expect(isAllOff(boardWith({ '0,1,0': LEVER_ON }))).toBe(false)
    expect(isAllOff(boardWith({ '1,1,0': DUST_LINE(1) }))).toBe(false)
    expect(isAllOff(boardWith({ '1,1,0': DUST_LINE(15) }))).toBe(false)
    expect(isAllOff(boardWith({ '2,1,0': LAMP(true) }))).toBe(false)
    expect(isAllOff(boardWith({ '2,1,0': 'minecraft:redstone_lamp[lit=true]' }))).toBe(false)
  })

  it('プロパティの無い lever・redstone_wire・redstone_lamp は切と読む (点いている印が無い)', () => {
    expect(isAllOff(boardWith({ '0,1,0': 'lever', '1,1,0': 'redstone_wire', '2,1,0': 'minecraft:redstone_lamp' }))).toBe(true)
    expect(isAllOff(boardWith({ '0,1,0': 'lever[]', '1,1,0': 'redstone_wire[east=side]', '2,1,0': 'redstone_lamp[' }))).toBe(true)
  })

  it('ほかのブロックの powered・lit は見ない', () => {
    expect(isAllOff(boardWith({ '0,1,0': 'stone_button[face=floor,facing=north,powered=true]', '1,1,0': 'furnace[facing=north,lit=true]' })))
      .toBe(true)
  })
})

// ── fixture を読む ────────────────────────────────────────────────────────

interface FixtureBlocks {
  readonly region: { readonly from: readonly number[]; readonly to: readonly number[] }
  readonly blocks: readonly { readonly pos: readonly [number, number, number]; readonly block: string }[]
}

function fixture(name: string): FixtureBlocks {
  return JSON.parse(readFileSync(new URL(`../../../conformance/fixtures/${name}.json`, import.meta.url), 'utf-8')) as FixtureBlocks
}
