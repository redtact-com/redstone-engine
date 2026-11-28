/**
 * テストからネットワークに出ない見張り (vitest の setupFiles。#11)。
 *
 * fetch と net の接続を包み、127.0.0.1・localhost・::1 のほかへ出ようとしたら断る。
 * - 外の取得先を使う所 (view-3d のテクスチャと JSON) は、テストで fetch を差し替える (vi.stubGlobal)。
 *   差し替えている間はここを通らず、vi.unstubAllGlobals で、この見張りに戻る
 * - engine-js の試験は 127.0.0.1 に立てた台から読むので通す
 * 見張り自身の試験は tools/ip-hygiene.test.ts にある。
 */

import net from 'node:net'

/** 出てよい先 (手元だけ) */
export const LOOPBACK = /^(127\.\d{1,3}\.\d{1,3}\.\d{1,3}|localhost|::1|\[::1\])$/i

/** 断った時の文の頭 (試験で見る) */
export const BLOCKED = 'テストからネットワークに出ない'

/** 行き先を見る URL の種類 (data: と blob: はネットワークに出ないので見ない) */
const NETWORK_PROTOCOLS = new Set(['http:', 'https:', 'ws:', 'wss:'])

/** ネットワークに出る URL なら行き先の host。出ない URL (data:・blob:・相対) は null */
function hostOf(url: string): string | null {
  try {
    const u = new URL(url)
    return NETWORK_PROTOCOLS.has(u.protocol) ? u.hostname : null
  } catch {
    return null   // 相対の URL などは、本物の fetch がそのまま断る
  }
}

const realFetch = globalThis.fetch
globalThis.fetch = function guardedFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
  const host = hostOf(url)
  if (host !== null && !LOOPBACK.test(host)) {
    return Promise.reject(new TypeError(`${BLOCKED} (${url})。fetch を差し替える (vi.stubGlobal)`))
  }
  return realFetch(input, init)
}

type Connect = (this: net.Socket, ...args: unknown[]) => net.Socket
const realConnect = net.Socket.prototype.connect as unknown as Connect

/**
 * 文字列が pipe (UNIX のソケット・名前付きパイプ) の名前か。net と同じ見分けで、0 以上の数として読めれば
 * port (空文字も port 0)、読めなければ pipe
 */
function isPipeName(s: string): boolean {
  return !(Number(s) >= 0)
}

/** connect の引数から行き先の host を出す。pipe (UNIX のソケット・名前付きパイプ) は null */
export function connectHost(args: readonly unknown[]): string | null {
  const first = args[0]
  if (Array.isArray(first)) return connectHost(first)
  if (typeof first === 'object' && first !== null) {
    // net と同じく、path が空でない文字列の時だけ pipe。http・https の Agent は path: null を付けて渡す
    const o = first as { host?: string | null; path?: string | null }
    if (o.path) return null
    return o.host || 'localhost'
  }
  if (typeof first === 'string' && isPipeName(first)) return null   // connect(path)
  return typeof args[1] === 'string' && args[1] !== '' ? args[1] : 'localhost'
}

net.Socket.prototype.connect = function guardedConnect(this: net.Socket, ...args: unknown[]): net.Socket {
  const host = connectHost(args)
  if (host !== null && !LOOPBACK.test(host)) {
    process.nextTick(() => this.destroy(new Error(`${BLOCKED} (${host} へ接続しようとした)`)))
    return this
  }
  return realConnect.apply(this, args)
} as unknown as typeof net.Socket.prototype.connect
