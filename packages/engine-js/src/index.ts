/**
 * engine-js: wasm のエンジン (crates/rse-wasm) を JS から使う窓口。
 *
 * ```ts
 * const engine = await loadEngineFromUrls(wasmUrl, statesUrl)   // Node は './node' の loadEngineFromFiles
 * const world = engine.createWorld()
 * world.load([{ x: 0, y: 0, z: 0, state: engine.resolve('stone') }])
 * world.step()
 * const w = world.changes()            // 線形メモリの窓。次に窓口を呼ぶ前に読む
 * for (let i = 0; i < w.count; i++) engine.states.name(w.state(i))
 * world.dispose()
 * ```
 *
 * 窓口の表と決まりは docs/wasm-abi.md にある。
 */

export * from './abi'
export { Engine, loadEngineFromUrls, type ResolveResult, type WasmSource } from './engine'
export { STATE_TABLE_FORMAT, StateTable, type BitName, type BlockInfo, type PropInfo, type StateTableJson } from './states'
export { ChangeWindow, RustWorld, type Cell } from './world'
