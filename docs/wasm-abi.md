# wasm の窓口 (C ABI)

エンジンを JS へ出す窓口の決まり。設計の元は redstone-sim の docs/research/19 §2.5 (JS との境界) で、
作りは crates/rse-wasm (Rust) と packages/engine-js (TS) にある (#9)。

- 素の C ABI。wasm-bindgen を使わない。**wasm の import は 0 個**で、Node・ブラウザ・Worker で同じ
  `WebAssembly.instantiate(bytes, {})` で読める。
- 世界はハンドル (番号) で持つ。インスタンス 1 つに世界をいくつも持てる。
- 文字列が境界を越えるのは名前の解決 (`rse_resolve`) だけ。状態は StateId (u16) で渡す。
- エラーは状態コード (ビットの集合) で返す。黙って捨てない。文は JS 側の表 (packages/engine-js/src/abi.ts) で引く。
- 版は `ABI_VERSION` = 1。関数の形・記録の並び・状態コードのビットを変えたら、Rust と TS の両方で上げる。

## 1. 関数 (7 関数 + 1)

19 §2.5 の 7 関数に、JS が書く所を返す `rse_scratch` を 1 つ足した。版を返す `rse_abi_version` は前からある。
ポインタは wasm32 の番地 (JS には i32 で来るので `>>> 0` で読む)。

| 関数 | 引数 | 返り値 | 返す状態コード |
|---|---|---|---|
| `rse_abi_version()` | — | 窓口の版 (1) | — |
| `rse_scratch(len)` | 広げる大きさ (バイト) | 作業領域の先頭の番地。割り当てに失敗したら 0 | — |
| `rse_world_new()` | — | ハンドル。席が尽きたら 0 | — |
| `rse_world_free(h)` | ハンドル | 状態コード | HANDLE (無い・2 回目) |
| `rse_resolve(ptr, len, out)` | UTF-8 の名前の番地と長さ・結果の番地 | 状態コード。成功なら `out` に StateId を u32 で書く | ARGUMENT・UNKNOWN_BLOCK・UNKNOWN_PROPERTY・UNKNOWN_VALUE・SYNTAX |
| `rse_load(h, recs, n, mode)` | ハンドル・マスの記録の番地と件数・読み込みの型 | 状態コード | HANDLE・ARGUMENT・OUT_OF_WORLD・UNIMPLEMENTED (REBUILD) |
| `rse_step(h, n)` | ハンドル・tick の数 | その間に立ったエンジンの状態コード | HANDLE・エンジンの全部 |
| `rse_input(h, kind, section, x, y, z, arg)` | ハンドル・入力の種類・区間の印・座標・種類ごとの値 | 連鎖を回し切った後の状態コード | HANDLE・ARGUMENT・エンジンの全部 |
| `rse_changes(h, len_out)` | ハンドル・件数の番地 | 変化の列 (マスの記録) の先頭の番地。失敗したら 0 | (0 を返す時は HANDLE か、`len_out` が作業領域の外) |

- `rse_resolve` は世界に依らないので、ハンドルを取らない。名前の形は rse-core の `StateId::parse` のとおり
  (先頭の `minecraft:` は読み飛ばし、書いていないプロパティは既定の値)。失敗した時は `out` に書かない。
- `rse_load` の `mode`: 0 = SETTLED (更新を出さずに置く。19 §4.2 の「そのまま」)・1 = REBUILD (席だけ。UNIMPLEMENTED を返す)。
  StateId が表の外 (ARGUMENT) か高さの外 (OUT_OF_WORLD) のマスがあれば、1 つも置かない。
  格子の枠の外 (離れすぎた 2 点) は置いてみるまで分からないので、そこで止めて OUT_OF_WORLD を返し、途中まで置いた世界が残る。
  JS はこの時、世界を捨てて作り直す。置いたマスは変化の列に載る。
- `rse_input` の `kind`: 0 = 使う (`arg` は 0。0 でなければ ARGUMENT)・1 = setblock (`arg` の下の 16 ビットが StateId、
  ビット 16 が strict。席だけで UNIMPLEMENTED を返す)。`section` は区間の印 (0 = B5・1 = C・2 = A・3 = B1。19 §4.1)。
  区間の印は順を変えない (呼んだ順に適用する)。受け付けたか (accepted) は返さない。変わったマスは変化の列で分かる。
- `rse_changes` は、前に取り出した後の正味の変化 (19 §3.8) を取り出す。`len_out` が作業領域の外なら、変化を取り出さずに 0 を返す
  (取り出した変化を捨てない)。ハンドルが無ければ 0 を返し、`len_out` に 0 を書く。空の列でも 0 でない番地を返す。
- 乱数の種 (19 §2.5 の例の `rse_world_new(seed_lo, seed_hi)`) は、乱数を使う素子が入る PR で足し、版を上げる。

## 2. 記録の並び

マスの記録 (`CellRec`) は、読み込みの列と変化の列で同じ形にした。

| 位置 (バイト) | 欄 | 型 |
|---:|---|---|
| 0 | x | i32 |
| 4 | y | i32 |
| 8 | z | i32 |
| 12 | state | u32 (StateId を 32 ビットに広げた物) |

- `#[repr(C)]`・16 バイト・4 バイト揃え・詰め物なし。JS は `Int32Array` の窓で 4 つずつ読む (state は 65,535 以下なので符号は立たない)。
- wasm の線形メモリはリトルエンディアン。

## 3. 状態コード

32 ビットの集合。下の 16 ビットはエンジンの状態コード (rse-core の `Status`・19 §2.5 の表)、上の 16 ビットは窓口の状態コード。

| ビット | 名前 | 意味 | engine-js の扱い |
|---:|---|---|---|
| 0 | CHAIN_LIMIT | 近隣更新の連鎖が上限に達し、新しい分を捨てた | 値で返す |
| 1 | TICK_CARRYOVER | 予約 tick が 1 tick の上限に達し、持ち越した | 値で返す |
| 2 | LOADED_ORDER_TIE | 別のチャンクの予約どうしが同点になった | 値で返す |
| 3 | TREE_BIN | HashMap の写しが木構造の条件に達した | 値で返す |
| 4 | UNIMPLEMENTED | 未実装の経路を通った | 値で返す |
| 5 | SAFETY | 安全弁が働き、実機と違う結果になった | 値で返す |
| 16 | HANDLE | 世界のハンドルが無い | 投げる |
| 17 | ARGUMENT | 引数が決まりの外 (作業領域の外・揃っていない・番号が無い・StateId が表の外) | 投げる |
| 18 | OUT_OF_WORLD | 置けない (高さの外・格子の枠の外) | 投げる |
| 19 | UNKNOWN_BLOCK | 状態表に無いブロックの名前 (19 §11 の 5) | 投げる (`tryResolve` は値で返す) |
| 20 | UNKNOWN_PROPERTY | そのブロックに無いプロパティ | 同上 |
| 21 | UNKNOWN_VALUE | そのプロパティに無い値 | 同上 |
| 22 | SYNTAX | 名前の形が崩れている (UTF-8 でない時も) | 同上 |

- エンジンの状態コードは「実機と違う結果になりうる」の知らせなので、呼び出し元へ値で返す。fixture の実行器は不一致として数える。
- 窓口の状態コードは使い方の誤りなので、engine-js は `EngineError` を投げる。

## 4. 線形メモリの扱い

- **JS が書いてよい所は作業領域だけ**。`rse_scratch(len)` が `len` バイト以上に広げ (縮めない)、先頭の番地を返す。
  先頭は 8 バイトに揃い、広げた分は 0 で埋まる。
- ポインタの引数 (`rse_resolve` の `ptr`・`out`、`rse_load` の `recs`、`rse_changes` の `len_out`) は、作業領域の内でなければ
  ARGUMENT を返し (`rse_changes` は 0 を返し)、何も読み書きしない。u32 とマスの記録の番地は 4 バイトに揃うこと。
  JS の誤りで、エンジンの値を黙って壊さない。
- 作業領域を広げる (中身を移す) のは `rse_scratch` だけ。広げると前の番地は使えないので、JS は `rse_scratch` を呼ぶたびに番地を取り直す。
- **型付き配列の窓は、窓口を呼んだ後に作り直す**。窓口の関数は割り当てで線形メモリを伸ばすことがあり (`rse_scratch`・
  `rse_world_new`・`rse_load`・`rse_step`・`rse_input`・`rse_changes`)、伸びると前の ArrayBuffer は切り離される (長さ 0)。
  engine-js は呼ぶたびに `memory.buffer` から窓を作り、次に呼ぶ前に使い終える。
- 変化の窓 (`ChangeWindow`) は線形メモリそのもので、写しを作らない。engine-js は窓口を呼んだ回数 (epoch) を数え、
  次に窓口を呼んだ後に窓を読むと例外にする。続けて使う値は、その前に読み出す。
- 変化の列 (Rust 側の Vec) は、次にその世界の `rse_changes` を呼ぶか世界を捨てるまで動かない。
- SharedArrayBuffer と Worker は使わない (19 §11 の 14)。

## 5. unsafe の置き場

- unsafe のブロックは crates/rse-wasm/src/scratch.rs だけ (作業領域の読み書きの 3 つ)。どれも `SAFETY:` で前提を書き、
  clippy の `undocumented_unsafe_blocks` と `multiple_unsafe_ops_per_block` で見張る。
- JS から来た番地は数としてだけ使い、作業領域の内かを確かめてから、作業領域の Vec 自身のポインタに位置を足して読み書きする。
- `#[unsafe(no_mangle)]` は crates/rse-wasm/src/exports.rs だけ。ほかの crate は `#![forbid(unsafe_code)]`。
  tools/ip-hygiene.test.ts がこの置き場を見張る。
- rse-wasm は std のまま組む。割り当て器 (std の dlmalloc) と、世界の表を持つ thread_local を std から借りる。
  no_std にすると、割り当て器を自分で書く (unsafe が増える) か、外部の crate を足すことになる。

## 6. 状態表の JSON

rse-cli の `rse states <ファイル>` が書き出す (crates/rse-cli/src/states.rs)。生成物なので追跡しない。

| 欄 | 中身 |
|---|---|
| `format` | 形の版 (1)。engine-js が確かめる |
| `stateCount` | 状態の総数 |
| `flagBits`・`statusBits` | 性質のビットと、エンジンの状態コードのビットの名前 |
| `blocks` | ブロックごとの名前・振る舞いの種類・最初の StateId・状態の数・既定の StateId・プロパティ |
| `states.name`・`states.flags` | StateId の順の blockstate の文字列 (プロパティは名前順で全部書く) と性質のビット |

- engine-js は読み込みの時に、各ブロックの最初と最後の状態の名前を wasm で解決し直し、JSON が wasm と同じビルドの物かを確かめる。

## 7. 作り方と読み込み方

| 場面 | やり方 |
|---|---|
| 作る | `npm run build:engine` (packages/engine-js/scripts/build.mjs)。cargo で wasm を組み、rse-cli で状態表の JSON を書き、どちらも packages/engine-js/generated/ に置く。`npm test` の前 (pretest) にも毎回走るので、テストが古い wasm で回らない |
| Node | `@redstone-engine/engine-js/node` の `loadEngineFromFiles()`。既定で generated/ から読む |
| ブラウザ | `loadEngineFromUrls(wasmUrl, statesUrl)`。wasm は `instantiateStreaming` で読む (MIME 型が違えばバイト列で読み直す)。URL はアプリが決める |
| どちらでも | `Engine.load(bytes か Module か Response, 状態表の JSON)` |
| CI | js の job で Rust の toolchain を入れ、`npm test` (pretest で wasm と JSON を作る) と `npm run size` (大きさの報告。合否にしない) を回す |
