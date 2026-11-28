import { describe, it, expect } from 'vitest'
import { fitDistance } from '../camera'
import {
  viewMatrixOf, projMatrixOf, canvasRay, pointToCanvas, pickCell, pickRoleFrom, pickNearest,
  PICK_PASS_THROUGH, TOUCH_PICK_RADII,
} from './picking'
import type { CameraPose, Cell3, Ray, Size3 } from './picking'

/**
 * 3D 盤面のピッキング (redstone-sim#397) の計算を確かめる。
 *
 * 描画 (WebGL) は vitest で動かせないので、**描画と同じ行列**で
 * 「マスの中心を画面に出す → その点から光線を出す → 同じマスに戻る」往復を見る。
 * 描画とピックが同じ viewMatrixOf / projMatrixOf を使っているので、
 * ここで往復が合えば、画面で押した所のマスが返る。
 */

const W = 800
const H = 600

function poseOf(rotX: number, rotY: number, size: Size3, pan: [number, number] = [0, 0]): CameraPose {
  return { rotX, rotY, distance: fitDistance(size, { aspect: W / H }), panX: pan[0], panZ: pan[1] }
}

function rayAt(cam: CameraPose, size: Size3, px: number, py: number): Ray {
  const ray = canvasRay(px, py, W, H, viewMatrixOf(cam, size), projMatrixOf(W / H))
  if (!ray) throw new Error('光線を作れない')
  return ray
}

function project(cam: CameraPose, size: Size3, p: readonly number[]): { x: number; y: number } {
  const c = pointToCanvas(p, W, H, viewMatrixOf(cam, size), projMatrixOf(W / H))
  if (!c) throw new Error(`カメラの後ろ: ${p.join(',')}`)
  return c
}

/** マスの中心を通る光線 */
function rayThroughCell(cam: CameraPose, size: Size3, cell: readonly number[]): Ray {
  const c = project(cam, size, [cell[0] + 0.5, cell[1] + 0.5, cell[2] + 0.5])
  return rayAt(cam, size, c.x, c.y)
}

const key = (c: readonly number[]) => `${c[0]},${c[1]},${c[2]}`

/** テストの「押せる物」: アプリが状態表の性質で決める所の代わり。名前で決める */
const TARGETS = new Set(['lever', 'stone_button', 'oak_door'])
const pickRole = pickRoleFrom(state => TARGETS.has(state.split('[')[0]))

/** 盤面 (鍵 → blockstate の文字列) のマスの扱い。空のマスは null (IsometricView と同じく pickRole を呼ばない) */
function roleFrom(blocks: Map<string, string>) {
  return (x: number, y: number, z: number) => {
    const s = blocks.get(`${x},${y},${z}`)
    return s === undefined ? null : pickRole(s)
  }
}

const LEVER = 'lever[face=floor,facing=north,powered=false]'
const WIRE = 'redstone_wire[east=side,north=none,power=0,south=none,west=side]'

const CAMERAS: [string, (s: Size3) => CameraPose][] = [
  ['既定 45/45', s => poseOf(45, 45, s)],
  ['ほぼ真上 80/45', s => poseOf(80, 45, s)],
  ['ほぼ真横 5/0', s => poseOf(5, 0, s)],
  ['下から 175/300', s => poseOf(175, 300, s)],
  ['真上 90/0 (topDown と同じ)', s => poseOf(90, 0, s)],
  ['パンあり 60/120', s => poseOf(60, 120, s, [2.3, -1.7])],
]

describe('往復: マスの中心を画面に出し、その点の光線で同じマスが返る', () => {
  for (const size of [[16, 16, 16], [7, 3, 12]] as Size3[]) {
    for (const [name, mk] of CAMERAS) {
      it(`${size.join('x')} / ${name}`, () => {
        const cam = mk(size)
        const wrong: string[] = []
        for (let x = 0; x < size[0]; x++) {
          for (let y = 0; y < size[1]; y++) {
            for (let z = 0; z < size[2]; z++) {
              const ray = rayThroughCell(cam, size, [x, y, z])
              // そのマスだけを当たりにする (ほかは空)
              const hit = pickCell(ray, size, (cx, cy, cz) =>
                cx === x && cy === y && cz === z ? 'target' : null)
              if (!hit || key(hit) !== key([x, y, z])) wrong.push(`${x},${y},${z} → ${hit ? key(hit) : 'null'}`)
            }
          }
        }
        expect(wrong).toEqual([])
      })
    }
  }
})

/** 再現できる乱数 (mulberry32) */
function seeded(seed: number): () => number {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** 光線 (t >= 0) がマス [c, c+1) を通る区間。入る t と長さ (通らなければ 0) */
function throughCell(ray: Ray, c: readonly number[]): { tIn: number; len: number } {
  let t0 = 0, t1 = Infinity
  for (let i = 0; i < 3; i++) {
    const o = ray.origin[i], d = ray.dir[i]
    if (Math.abs(d) < 1e-12) {
      if (o < c[i] || o >= c[i] + 1) return { tIn: 0, len: 0 }
      continue
    }
    let ta = (c[i] - o) / d, tb = (c[i] + 1 - o) / d
    if (ta > tb) [ta, tb] = [tb, ta]
    t0 = Math.max(t0, ta)
    t1 = Math.min(t1, tb)
  }
  return { tIn: t0, len: Math.max(0, t1 - t0) }
}

describe('格子の辿り方', () => {
  it('光線が通るマスを、手前から順に、過不足なく辿る (総当たりと突き合わせる)', () => {
    // 往復のテストは「目的のマスに届くか」しか見ないので、余計なマスを辿っても通ってしまう。
    // 余計なマスが押せる素子なら、押していない素子が切り替わる。ここで 1 マスずつ突き合わせる
    const rnd = seeded(397)
    const size: Size3 = [16, 16, 16]
    let hits = 0
    const inBox = () => [rnd() * size[0], rnd() * size[1], rnd() * size[2]]
    for (let n = 0; n < 300; n++) {
      let ray: Ray
      if (n % 3 === 0) {
        // カメラから、箱の中の適当な点が描かれる画素へ
        const cam = poseOf(5 + rnd() * 170, rnd() * 360, size)
        const c = project(cam, size, inBox())
        ray = rayAt(cam, size, c.x, c.y)
      } else {
        // 箱の中や外の適当な点から、箱の中の点へ向けて / 適当な向きへ (箱を外れるものも混ぜる)
        const o: [number, number, number] = [rnd() * 40 - 12, rnd() * 40 - 12, rnd() * 40 - 12]
        const to = n % 3 === 1 ? inBox() : [o[0] + rnd() - 0.5, o[1] + rnd() - 0.5, o[2] + rnd() - 0.5]
        const d = [to[0] - o[0], to[1] - o[1], to[2] - o[2]]
        const len = Math.hypot(d[0], d[1], d[2])
        ray = { origin: o, dir: [d[0] / len, d[1] / len, d[2] / len] }
      }
      const walked: string[] = []
      pickCell(ray, size, (x, y, z) => { walked.push(key([x, y, z])); return null })

      const expected: { k: string; t: number }[] = []
      for (let x = 0; x < size[0]; x++) {
        for (let y = 0; y < size[1]; y++) {
          for (let z = 0; z < size[2]; z++) {
            const h = throughCell(ray, [x, y, z])
            if (h.len > 1e-9) expected.push({ k: key([x, y, z]), t: h.tIn })
          }
        }
      }
      expected.sort((a, b) => a.t - b.t)
      expect(walked, `ray #${n}`).toEqual(expected.map(e => e.k))
      if (walked.length > 0) hits++
    }
    // 箱に当たらない光線ばかりだと確かめたことにならない
    expect(hits).toBeGreaterThan(150)
  })
})

describe('手前のマスの扱い (素通り・遮る)', () => {
  const size: Size3 = [16, 16, 16]
  const lever: Cell3 = [8, 1, 8]

  for (const [name, mk] of CAMERAS) {
    it(`${name}: 光線上でレバーの手前にあるマスがワイヤーなら素通り、石なら遮る`, () => {
      const cam = mk(size)
      const ray = rayThroughCell(cam, size, lever)
      // 光線がレバーより前に通るマスを集める
      const before: Cell3[] = []
      pickCell(ray, size, (x, y, z) => {
        if (key([x, y, z]) === key(lever)) return 'target'
        before.push([x, y, z])
        return null
      })
      expect(before.length).toBeGreaterThan(0)
      const front = before[before.length - 1]

      const blocks = new Map<string, string>([[key(lever), LEVER]])
      const roleAt = roleFrom(blocks)
      for (const t of [WIRE, 'redstone_torch[lit=true]', 'repeater[delay=1,facing=north,locked=false,powered=false]',
        'rail[shape=north_south,waterlogged=false]', 'glass', 'lime_stained_glass_pane[east=false,north=true,south=true,waterlogged=false,west=false]',
        'water[level=0]']) {
        blocks.set(key(front), t)
        expect(pickCell(ray, size, roleAt), `${t} は素通り`).toEqual(lever)
      }
      for (const t of ['stone', 'smooth_stone_slab[type=bottom,waterlogged=false]', 'redstone_lamp[lit=false]', 'piston[extended=false,facing=up]', 'glowstone']) {
        blocks.set(key(front), t)
        expect(pickCell(ray, size, roleAt), `${t} は遮る`).toBeNull()
      }
      // 手前が押せる素子なら、そちらが当たる (奥のレバーではない)
      blocks.set(key(front), 'stone_button[face=floor,facing=north,powered=false]')
      expect(pickCell(ray, size, roleAt)).toEqual(front)
    })
  }

  it('軸に沿った光線: 手前のワイヤーは素通り、石の奥のレバーは null', () => {
    const ray: Ray = { origin: [5.5, 1.5, -3], dir: [0, 0, 1] }
    const blocks = new Map<string, string>([
      ['5,1,5', LEVER],
      ['5,1,3', WIRE],
    ])
    expect(pickCell(ray, size, roleFrom(blocks))).toEqual([5, 1, 5])
    blocks.set('5,1,3', 'stone')
    expect(pickCell(ray, size, roleFrom(blocks))).toBeNull()
  })

  it('奥の素子より手前の素子が先に当たる', () => {
    const ray: Ray = { origin: [5.5, 1.5, -3], dir: [0, 0, 1] }
    const blocks = new Map<string, string>([
      ['5,1,9', LEVER],
      ['5,1,4', 'oak_door[facing=north,half=lower,hinge=left,open=false,powered=false]'],
    ])
    expect(pickCell(ray, size, roleFrom(blocks))).toEqual([5, 1, 4])
  })
})

describe('外れ', () => {
  const size: Size3 = [16, 16, 16]
  const all = () => 'target' as const

  it('盤面の箱から外れた光線は null', () => {
    // 箱の横を素通りする
    expect(pickCell({ origin: [-5, 20, -5], dir: [0, -1, 0] }, size, all)).toBeNull()
    // 箱と反対を向いている
    expect(pickCell({ origin: [8, 20, 8], dir: [0, 1, 0] }, size, all)).toBeNull()
    // 斜めに外れる
    const d = Math.SQRT1_2
    expect(pickCell({ origin: [-10, 8, 30], dir: [d, 0, d] }, size, all)).toBeNull()
  })

  it('何も無い盤面を通り抜けたら null', () => {
    const cam = poseOf(45, 45, size)
    const ray = rayAt(cam, size, W / 2, H / 2)
    expect(pickCell(ray, size, () => null)).toBeNull()
  })

  it('画面の隅 (盤面の外) から出した光線は null', () => {
    const cam = poseOf(45, 45, size)
    expect(pickCell(rayAt(cam, size, 2, 2), size, all)).toBeNull()
  })
})

describe('軸に平行な光線でも 0 で割らない', () => {
  const size: Size3 = [16, 16, 16]

  it('真下向き: 上から順に見て最初の素子を返す', () => {
    const ray: Ray = { origin: [3.5, 40, 7.5], dir: [0, -1, 0] }
    const blocks = new Map<string, string>([
      ['3,2,7', LEVER],
      ['3,0,7', 'stone'],
    ])
    expect(pickCell(ray, size, roleFrom(blocks))).toEqual([3, 2, 7])
    blocks.set('3,5,7', 'stone')
    expect(pickCell(ray, size, roleFrom(blocks))).toBeNull()
  })

  it('真上のカメラ (90/0) の画面の中心', () => {
    // 中心がマスの境界に乗らないよう半マスずらす
    const cam = poseOf(90, 0, size, [0.5, 0.5])
    const ray = rayAt(cam, size, W / 2, H / 2)
    expect(ray.origin.every(Number.isFinite)).toBe(true)
    expect(ray.dir.every(Number.isFinite)).toBe(true)
    const blocks = new Map<string, string>([['8,0,8', LEVER]])
    expect(pickCell(ray, size, roleFrom(blocks))).toEqual([8, 0, 8])
  })
})

describe('pickRoleFrom: マスの扱いの分け方', () => {
  it('アプリが押せると言った物は当たり。素通りの一覧より先に見る', () => {
    expect(pickRole(LEVER)).toBe('target')
    expect(pickRole('stone_button[face=wall,facing=east,powered=true]')).toBe('target')
    // 一覧にある名前でも、アプリが押せると言えば当たり
    const wireTarget = pickRoleFrom(s => s.startsWith('redstone_wire'))
    expect(wireTarget(WIRE)).toBe('target')
  })

  it('素通りの一覧は名前で引く (プロパティは見ない)。色付きのガラスとガラス板・鉄格子・氷も素通り', () => {
    const none = pickRoleFrom(() => false)
    for (const name of PICK_PASS_THROUGH) expect(none(name), name).toBe('pass')
    for (const s of [WIRE, 'redstone_wall_torch[facing=east,lit=false]', 'comparator[facing=north,mode=compare,powered=false]',
      'black_stained_glass', 'white_stained_glass_pane[east=true,north=false,south=false,waterlogged=false,west=true]',
      'iron_bars[east=false,north=false,south=false,waterlogged=false,west=false]', 'ice', 'bubble_column[drag=true]']) {
      expect(none(s), s).toBe('pass')
    }
  })

  it('一覧に無い名前は遮る (安全側)。押せるかの意味は持たない (アプリが言わなければレバーも遮る)', () => {
    const none = pickRoleFrom(() => false)
    for (const s of ['stone', 'redstone_lamp[lit=true]', LEVER, 'glowstone', 'sea_lantern', 'unknown_block']) {
      expect(none(s), s).toBe('block')
    }
  })
})

describe('pickNearest: タッチの外れ補正', () => {
  it('押した点で当たれば、周りは探さない', () => {
    let calls = 0
    const hit = pickNearest(100, 100, () => { calls++; return 'A' }, () => ({ x: 100, y: 100 }), TOUCH_PICK_RADII)
    expect(hit).toBe('A')
    expect(calls).toBe(1)
  })

  it('外れたら周りの円周上を探し、画面上の中心が一番近いものを選ぶ', () => {
    const tried: [number, number][] = []
    const hit = pickNearest(
      100, 100,
      (x, y) => {
        tried.push([x, y])
        if (x > 105) return 'far'     // 右側: 中心が遠い素子
        if (y < 95) return 'near'     // 上側: 中心が近い素子
        return null
      },
      h => (h === 'far' ? { x: 130, y: 100 } : { x: 100, y: 92 }),
      TOUCH_PICK_RADII,
    )
    expect(hit).toBe('near')
    // 押した点 + 2 つの半径 × 8 方向
    expect(tried).toHaveLength(1 + TOUCH_PICK_RADII.length * 8)
    const radii = tried.slice(1).map(([x, y]) => Math.round(Math.hypot(x - 100, y - 100)))
    expect([...new Set(radii)].sort((a, b) => a - b)).toEqual([...TOUCH_PICK_RADII])
  })

  it('周りにも何も無ければ null。半径を渡さなければ押した点だけを見る', () => {
    expect(pickNearest(0, 0, () => null, () => null, TOUCH_PICK_RADII)).toBeNull()
    let calls = 0
    expect(pickNearest(0, 0, () => { calls++; return null }, () => null)).toBeNull()
    expect(calls).toBe(1)
  })
})
