# wasm の窓口 (C ABI)

エンジンを JS へ出す窓口の決まり。設計の元は redstone-sim の docs/research/19 §2.5 (JS との境界) で、
作りは crates/rse-wasm (Rust) と packages/engine-js (TS) にある (#9)。

- 素の C ABI。wasm-bindgen を使わない。**wasm の import は 0 個**で、Node・ブラウザ・Worker で同じ
  `WebAssembly.instantiate(bytes, {})` で読める。
- 世界はハンドル (番号) で持つ。インスタンス 1 つに世界をいくつも持てる。
- 文字列が境界を越えるのは名前の解決 (`rse_resolve`) だけ。状態は StateId (u16) で渡す。
- エラーは状態コード (ビットの集合) で返す。黙って捨てない。文は JS 側の表 (packages/engine-js/src/abi.ts) で引く。
- 版は `ABI_VERSION` = 2。関数の形・記録の並び・状態コードのビット・種類 / 区間 / 型の番号 (§1 の表)・ハンドルの形を
  変えたら、Rust と TS の両方で上げる。engine-js は、関数の顔ぶれより先に版を確かめる。
  テストは版の字面を書かず、`ABI_VERSION` から作る (engine-js のテストが、組んだ wasm の版を TS の版と比べる)。

| 版 | 中身 |
|---:|---|
| 1 | 19 §2.5 の 7 関数・作業領域・使う入力 (#9) |
| 2 | 置く・壊すの入力 (`kind` の 2 と 3)・時計を読む `rse_game_time` (#13) |

## 1. 関数 (10 関数)

19 §2.5 の 7 関数に、JS が書く所を返す `rse_scratch` と、時計を読む `rse_game_time` (版 2) を足した。
版を返す `rse_abi_version` は前からある。書き出す関数は全部で 10 個。
ポインタは wasm32 の番地 (JS には i32 で来るので `>>> 0` で読む)。

| 関数 | 引数 | 返り値 | 返す状態コード |
|---|---|---|---|
| `rse_abi_version()` | — | 窓口の版 (2) | — |
| `rse_scratch(len)` | 広げる大きさ (バイト) | 作業領域の先頭の番地。割り当てに失敗したら 0 | — |
| `rse_world_new()` | — | ハンドル。席が尽きたら 0 | — |
| `rse_world_free(h)` | ハンドル | 状態コード | HANDLE (無い・2 回目) |
| `rse_resolve(ptr, len, out)` | UTF-8 の名前の番地と長さ・結果の番地 | 状態コード。成功なら `out` に StateId を u32 で書く | ARGUMENT・UNKNOWN_BLOCK・UNKNOWN_PROPERTY・UNKNOWN_VALUE・SYNTAX |
| `rse_load(h, recs, n, mode)` | ハンドル・マスの記録の番地と件数・読み込みの型 | 状態コード | HANDLE・ARGUMENT・OUT_OF_WORLD・UNIMPLEMENTED (REBUILD) |
| `rse_step(h, n)` | ハンドル・tick の数 | その間に立ったエンジンの状態コード | HANDLE・エンジンの全部 |
| `rse_input(h, kind, section, x, y, z, arg)` | ハンドル・入力の種類・区間の印・座標・種類ごとの値 | 連鎖を回し切った後の状態コード | HANDLE・ARGUMENT・OUT_OF_WORLD (置く)・エンジンの全部 |
| `rse_changes(h, len_out)` | ハンドル・件数の番地 | 変化の列 (マスの記録) の先頭の番地。失敗したら 0 | (0 を返す時は HANDLE か、`len_out` が作業領域の外) |
| `rse_game_time(h)` | ハンドル | ゲーム時刻 (i64)。ハンドルが無ければ -1 | (-1 を返す時は HANDLE) |

番号の表 (どれも版に入る):

| 引数 | 番号 |
|---|---|
| `rse_load` の `mode` | 0 = SETTLED (更新を出さずに置く。19 §4.2 の「そのまま」)・1 = REBUILD (席だけ。UNIMPLEMENTED を返す) |
| `rse_input` の `kind` | 0 = 使う (`arg` は 0)・1 = setblock (`arg` の下の 16 ビットが StateId、ビット 16 が strict。席だけで UNIMPLEMENTED を返す)・2 = 置く (`arg` の下の 16 ビットが StateId、上の 16 ビットは 0)・3 = 壊す (`arg` は 0) |
| `rse_input` の `section` | 0 = B5・1 = C・2 = A・3 = B1 (19 §4.1。rse-core の InputSection の値) |

- `rse_resolve` は世界に依らないので、ハンドルを取らない。名前の形は rse-core の `StateId::parse` のとおり
  (先頭の `minecraft:` は読み飛ばし、書いていないプロパティは既定の値)。失敗した時は `out` に書かない。
- `rse_load` は、StateId が表の外 (ARGUMENT) か、高さの外・格子の枠の外 (OUT_OF_WORLD) のマスがあれば、**1 つも置かない**。
  格子の枠は、置く前に rse-core (`Level::can_set_all_raw`) で、今の格子と列の座標を合わせた箱が 1 辺 256 セクションに収まるかを確かめる
  (空気のマスは格子を広げないので数えない)。断った読み込みは、マスの状態も、格子の広がりもセクションも残さない。置いたマスは変化の列に載る。
- `rse_input`: 区間の印は順を変えない (呼んだ順に適用する)。受け付けたか (accepted) は返さない。変わったマスは変化の列で分かる。
  種類ごとの断りは下の表のとおり。

| 種類 | ARGUMENT | OUT_OF_WORLD | 何もせずに 0 | UNIMPLEMENTED |
|---|---|---|---|---|
| 使う | `arg` が 0 でない | — | 何もしない所を使う | 族を外した組み立ての素子 (何も書かない) |
| setblock | StateId が表の外・ビット 17 以上 | — | — | いつも (席だけ。何も書かない) |
| 置く | StateId が表の外・空気・上の 16 ビットが 0 でない | 高さの外・格子の枠の外 | 置き換えられない所 (空気でない所)・支えの無い所 | 規則の無い種類 (族を外した組み立ての素子・族の無い種類・プロパティを持つ材料)・入った状態のレバーの要求 (何も書かない) |
| 壊す | `arg` が 0 でない | — (範囲は確かめない) | 空気を壊す (高さの外・格子の枠の外も空気) | 族を外した組み立ての素子の撤去 (マスは空気に書いてから、撤去フックで立つ) |

- 状態コードは、連鎖の中で立った物も含む。だから UNIMPLEMENTED が返っても、書いていることがある (上の壊すの行や、
  置いたブロックの周りの素子が更新を受けて立てた時)。変わったマスは、状態コードでなく変化の列で見る。

- 置く (プレイヤーの設置。区間 A。V01 §4.10): `arg` の StateId は、置くブロックと、プレイヤーが選ぶ値 (レバーの face と facing) を運ぶ。
  周りで決まる値 (ダストの 4 方向と power・ランプの lit) は、置いた時の規則でエンジンが決めるので読まない (ダストとランプは既定の状態を渡せばよい)。
  レバーの powered は偽で渡す (UNVERIFIED: 真の要求は書かずに UNIMPLEMENTED にしている。redstone-sim#446 の質問 2)。
  手順と典拠は crates/rse-core/src/input.rs の冒頭にある。
  - 高さの外と格子の枠の外は、置く前に `rse_load` と同じ確かめ (`Level::can_set_all_raw`) で断る。
    rse-core の置く手順は、格子の枠の外では書き込みまで進んで SAFETY を立てる (高さの外は何もせずに戻る) ので、窓口はそこへ渡さない。
    断った時は、世界も格子も変えず、状態コードを後に残さない。
- 壊す (プレイヤーの破壊。区間 A。V01 §4.10): そのマスを空気にする (`Level::remove_block`。撤去の処理は書き込みの中で走る)。
  範囲を確かめないのは、高さの外も格子の枠の外も読むと空気で、空気を壊しても何も起きないからである (rse-core の input.rs の冒頭)。
- setblock の `arg` (StateId 1 つ) では、26.3 の /setblock の「明記したプロパティだけを使い、ほかは周りに合わせる」(V06 §4.14.1) を運べない。
  中身を作る PR で `arg` の形を直し、版を上げる。
- `rse_game_time` は、L5 で 1 つ進むゲーム時刻 (V01 §2.3・§4.4) を返す。B2 の時計ではない。作った直後は 0 で、読み込みと入力では進まず、
  `rse_step(h, n)` で n 進む。0 から増えるだけなので、-1 (ハンドルが無い) は本物の値と重ならない。結果の番地を受けずに値で返すので、作業領域を使わない。
- `rse_changes` は、前に取り出した後の正味の変化 (19 §3.8) を取り出す。`len_out` が作業領域の外なら、変化を取り出さずに 0 を返す
  (取り出した変化を捨てない)。ハンドルが無ければ 0 を返し、`len_out` に 0 を書く。空の列でも 0 でない番地を返す。

19 §2.5 の例 (名前は仮) から外した所:

| 所 | 理由 |
|---|---|
| `rse_world_free` が状態コードを返す | 例は返り値を持たない。古いハンドルと 2 回目の解放を、黙って通さないため |
| `rse_world_new` が乱数の種を取らない | 乱数を使う素子がまだ無い。入る PR で足し、版を上げる |
| `rse_scratch` を足した | JS が書く所を wasm の中に決めるため (§4) |
| 壊す入力 (`kind` = 3) を足した (版 2) | 19 §2.5・§4.1 の入力の一覧に無かった。編集の間もエンジンの世界に置く・壊すを入れる形にしたので (#13)、プレイヤーの破壊も入力にする |
| 時計の口 `rse_game_time` を足した (版 2) | 画面の tick を、世界の時計と別に数えず、世界の時計そのものから出すため (#13) |
| 20 §3.1 の `currentTick` (読むだけの getter) を、engine-js では `gameTime()` のメソッドにした | 窓口を呼ぶので、前の変化の窓を古くする (§4)。読むだけに見える getter にしない |
| 書き出す関数が 10 個 | 19 §2.5 の 7 関数・`rse_scratch`・`rse_abi_version`・`rse_game_time`。engine-js のテストが、書き出しの全部 (10 関数・memory・下の 2 つ) を固定する |
| 書き出しに global が 2 つある (`__data_end`・`__heap_base`) | Rust の wasm32 のリンカが cdylib に出す物で、JS は使わない |

- 版 2 で外した 3 つ (壊す・時計の口・gameTime のメソッド) は、redstone-sim の 19 §2.5 の側にも記録する (redstone-sim#446 の質問 5)。

### ハンドル

- 32 ビット。**下の 16 ビットが席の番号 + 1、上の 16 ビットが席の世代**。0 は、どの世界のハンドルにもならない。
- 捨てた席は使い回し、世代を 1 つ進める。世代が最後 (0xFFFF) の世界を捨てた席は、使い回さずに退かせる。
  世代は一周しないので、同じハンドルが 2 つの世界を指すことは無く、捨てた世界の古いハンドルはいつまでも HANDLE を返す。
- 限り: 同時に持てる世界は 65,535 個。作れる世界の延べの数は 65,535 席 × 65,536 世代で、使い切ると `rse_world_new` は 0 を返す
  (インスタンスを読み直すまで)。
- engine-js の RustWorld は、捨てた後の操作を wasm に渡さずに EngineError (HANDLE) で断る。

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
下の表の「番号」はビットの番号で、値は `1 << 番号` (状態表の JSON の `bit` と engine-js の `STATUS` は値で持つ)。

| 番号 | 名前 | 意味 | engine-js の扱い |
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
| 19 | UNKNOWN_BLOCK | 状態表に無いブロックの名前 (19 §11 の 5) | `resolve` は投げる |
| 20 | UNKNOWN_PROPERTY | そのブロックに無いプロパティ | 同上 |
| 21 | UNKNOWN_VALUE | そのプロパティに無い値 | 同上 |
| 22 | SYNTAX | 名前の形が崩れている (UTF-8 でない時も) | 同上 |

- エンジンの状態コードは「実機と違う結果になりうる」の知らせなので、呼び出し元へ値で返す。fixture の実行器は不一致として数える。
- 窓口の状態コードは使い方の誤りなので、engine-js は `EngineError` を投げる。ただし `tryResolve` は、`rse_resolve` が返した
  窓口の状態コード (UNKNOWN_BLOCK ほか・ARGUMENT) を全部値で返し、投げない。
- engine-js は、表に無い名前 (型・種類・区間。継いだ鍵の `toString` なども) と範囲の外の数 (座標・tick の数・`arg`・`place` の StateId) を、
  wasm に渡す前に `RangeError` で断る (番号 0 として通さない)。`place` の StateId は 0〜0xFFFF の整数であること
  (表の外と空気は、wasm が ARGUMENT で断る)。捨てた世界の操作は、引数を見る前に、wasm を呼ばずに `EngineError` (HANDLE) で断る。
- `place`・`breakBlock` は `use` と同じく、エンジンの状態コードを値で返す (置けない所は 0・規則の無い要求は UNIMPLEMENTED)。
  OUT_OF_WORLD と ARGUMENT は窓口の状態コードなので投げる。

## 4. 線形メモリの扱い

- **JS が書いてよい所は作業領域だけ**。`rse_scratch(len)` が `len` バイト以上に広げ (縮めない)、先頭の番地を返す。
  先頭は 8 バイトに揃い、広げた分は 0 で埋まる。
- ポインタの引数 (`rse_resolve` の `ptr`・`out`、`rse_load` の `recs`、`rse_changes` の `len_out`) は、作業領域の内でなければ
  ARGUMENT を返し (`rse_changes` は 0 を返し)、何も読み書きしない。u32 とマスの記録の番地は 4 バイトに揃うこと。
  JS の誤りで、エンジンの値を黙って壊さない。
- 作業領域を広げる (中身を移す) のは `rse_scratch` だけ。広げると前の番地は使えないので、JS は `rse_scratch` を呼ぶたびに番地を取り直す。
- **型付き配列の窓は、窓口を呼んだ後に作り直す**。`rse_abi_version`・`rse_resolve`・`rse_game_time` のほかは全部、割り当てで線形メモリを
  伸ばしうる (`rse_world_free` も、空いた席の一覧を伸ばす)。伸びると前の ArrayBuffer は切り離される (長さ 0)。
  engine-js は呼ぶたびに `memory.buffer` から窓を作り、次に呼ぶ前に使い終える。
- `rse_game_time` は線形メモリを伸ばさず、作業領域も使わない。それでも窓口の呼び出しなので、engine-js の `gameTime()` は epoch を進め、
  前に作った変化の窓を古くする (epoch の決まりを、関数ごとに分けない)。**変化を読み切ってから時計を読む**。
- `rse_game_time` の返り値は i64 なので、JS には BigInt で来る。engine-js の `gameTime()` が number に直す。-1n は HANDLE を投げ、
  0 以上の安全な整数 (2^53 − 1 まで) の外は `RangeError` にする (1 tick ずつ進めて届く数ではない)。
- engine-js は窓口の関数を全部 1 つの口 (Core) から呼び、呼んだ回数 (epoch) を数える。1 つのインスタンス (線形メモリ) から作れる
  Engine は 1 つだけ (`Engine.fromExports` に同じ書き出しを 2 回渡すと断る)。2 つあると epoch が分かれ、片方で線形メモリが伸びても、
  もう片方の窓が古いと分からないため。
- 変化の窓 (`ChangeWindow`) は線形メモリそのもので、写しを作らない。次に窓口を呼んだ後に窓を読むと例外にする。
  epoch に頼らず、窓の配列の buffer が今の `memory.buffer` かも比べるので、外から `memory.grow` を呼んで切り離された時も読まずに断る。
  続けて使う値は、その前に読み出す。`data` は読むだけにし、持ち続けない。窓は自分の世界を持つので、窓が生きている間は
  世界が回収されない。添字が件数の外なら `RangeError`。
- 線形メモリの上の配列 (変化の窓の `data` など) を `load` に渡すと、作業領域を広げる前に写してから使う。
- 変化の列 (Rust 側の Vec) は、次にその世界の `rse_changes` を呼ぶか世界を捨てるまで動かない。
- dispose を忘れた世界は、安全網 (FinalizationRegistry) が回収の時に捨てる (19 §6.3)。安全網も Core を通すので、epoch が進む。
  回収の時の例外は外へ出さない。
- **trap はインスタンスが壊れた印で、Engine ごと捨てて読み直す**。release は panic = abort なので、wasm の中の panic
  (今の経路では割り当ての失敗) は trap (WebAssembly.RuntimeError) になり、その後は中の表の借用が戻らず、どの関数も trap し続ける。
  engine-js は trap を受けたら Engine を止まった状態 (`stopped`) にし、以後は wasm を呼ばずに `EngineStoppedError` を投げる。
  例外の文の頭は、呼んだ操作の名前 (`load`・`changes`・`resolve`・`use`・`place`・`breakBlock`・`gameTime`・`input (種類)` ほか。捨てた世界の HANDLE と、窓口の状態コードの例外も同じ)。止まった Engine では、`dispose` は投げずに終え
  (finally の中で元の例外を隠さない)、安全網も呼ばない。安全網の中で trap が起きた時も、投げずに Engine を止める。
- SharedArrayBuffer と Worker は使わない (19 §11 の 14)。

## 5. unsafe の置き場

- unsafe のブロックは crates/rse-wasm/src/scratch.rs だけ (作業領域の読み書きの 3 つと、テスト用の書き込みの 1 つ)。
  どれも `SAFETY:` で前提を書き、clippy の `undocumented_unsafe_blocks` と `multiple_unsafe_ops_per_block` で見張る。
- JS から来た番地は数としてだけ使い、作業領域の内かを確かめてから、作業領域の Vec 自身のポインタに位置を足して読み書きする。
  テストの読み書きも同じ確かめを通す (変化の列は、返した番地と件数がその世界の列そのものかを確かめてから写す)。
- `#[unsafe(no_mangle)]` は crates/rse-wasm/src/exports.rs だけ。unsafe_code を許すのは scratch.rs と exports.rs だけで、
  rse-wasm の lib.rs は crate 全体で deny する。ほかの crate は `#![forbid(unsafe_code)]`。
- tools/ip-hygiene.test.ts がこの置き場を見張る。注・文字列・文字の字面を外した文に当て、`unsafe` の字 (ブロック・関数・
  `cfg_attr` の中の属性・macro の引数) は scratch.rs (と exports.rs の属性) の外に 1 つも無いこと、`allow` / `expect` / `warn` の
  `unsafe_code` は、rustfmt が折った複数行の属性も含めて scratch.rs と exports.rs の外に無いことを見る。見張り自身のテストもある。
- doctest の中には crate の forbid が効かないので、rse-java・rse-core・rse-engine・rse-blocks-redstone の lib.rs に
  `#![doc(test(attr(forbid(unsafe_code))))]` を付けた (#13。PR #10 の「後で」)。今は doctest が無い。検査はそのまま残し、
  緩めるのは doctest を書く PR にする。同じ検査が、`///`・`//!` の中の囲み (``` か ~~~ を 3 つ以上。言葉が無いか、`,` と空白で割った言葉が全部
  rustdoc の印 (rust・no_run・edition2024 ほか)) と、空の行の後の 4 字以上の字下げのコードを doctest として落とす。
  `#[doc = "…"]` の形の doctest は拾わない。
- rse-wasm は std のまま組む。割り当て器 (std の dlmalloc) と、世界の表を持つ thread_local を std から借りる。
  no_std にすると、割り当て器を自分で書く (unsafe が増える) か、外部の crate を足すことになる。

## 6. 状態表の JSON

rse-cli の `rse states <ファイル>` が書き出す (crates/rse-cli/src/states.rs)。生成物なので追跡しない。

| 欄 | 中身 |
|---|---|
| `format` | 形の版 (1)。engine-js が確かめる |
| `stateCount` | 状態の総数 |
| `flagBits`・`statusBits` | 性質のビットと、エンジンの状態コードのビットの名前と値 (`bit` は `1 << 番号`) |
| `blocks` | ブロックごとの名前・振る舞いの種類・最初の StateId・状態の数・既定の StateId・プロパティ |
| `states.name`・`states.flags` | StateId の順の blockstate の文字列 (プロパティは名前順で全部書く) と性質のビット |

- engine-js は読み込みの時に、全部の状態の名前を wasm で解決し直して番号を比べ、ブロックの名前だけ (既定の状態) も比べる。
  合わなければ、別のビルドの JSON として断る。
- 性質のビットと、wasm 側にだけある状態 (JSON に無い末尾のブロック) は、この確かめでは分からない。状態表のハッシュを
  wasm から返して比べる形 (版が上がる) は後の PR で入れる。

## 7. 作り方と読み込み方

| 場面 | やり方 |
|---|---|
| 作る | `npm run build:engine` (packages/engine-js/scripts/build.mjs)。cargo で wasm を組み、rse-cli で状態表の JSON を書き、どちらも packages/engine-js/generated/ に置く。target の場所は `cargo metadata` に聞く。`npm test` の前 (pretest) にも毎回走るので、テストが古い wasm で回らない |
| Node | `@redstone-engine/engine-js/node` の `loadEngineFromFiles(dir?)`。vitest や束ねる道具 (Vite ほか) の中で読む入口で、素の node では読めない (パッケージの exports が .ts を指し、import に拡張子が無い)。node:fs を使うので、ブラウザの束には入れない。既定で generated/ から読む。`dir` は URL かファイルの道筋で、末尾の `/` は無くてよい |
| ブラウザ | `loadEngineFromUrls(wasmUrl, statesUrl)`。2 つを同時に取って両方を待つ (片方の失敗で、もう片方の reject を取り残さない。失敗は「wasm を取れない」「状態表の JSON が読めない」で包む)。wasm は `instantiateStreaming` で読む (MIME 型が違えばバイト列で読み直す)。URL はアプリが決める |
| どちらでも | `Engine.load(bytes か Module か Response, 状態表の JSON)`。インスタンスを自分で作ったら `Engine.fromExports(exports, 状態表の JSON)` |
| CI | js の job で Rust の toolchain を入れ、`npm test` (pretest で wasm と JSON を作る) と `npm run size` (大きさの報告。合否にしない) を回す |
