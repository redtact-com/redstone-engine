# 作りの決まり

設計の元は redtact-com/redstone-sim の docs/research/19 (Rust/WASM のエンジン) と 20 (止めた状態の書き出しと読み込み)。
この文書は、この repo での置き場・依存の向き・機能の足し方を決める。方針の経緯は #1 と #2 のコメントにある。

## 1. 置き場

| 置き場 | 役 | 依存してよい物 |
|---|---|---|
| crates/rse-java | Java の意味論の写し (方向の表・位置の詰め方とハッシュ値・HashSet の順・乱数) | なし |
| crates/rse-core | 骨格 (状態表・世界・書き込みと近隣更新・予約 tick・ブロックイベント・相・入力・信号の問い合わせ・変化の記録・状態コード) | rse-java |
| crates/rse-blocks-* | 素子の族。1 つ目は rse-blocks-redstone (レバー・ダスト・ランプ) | rse-core |
| crates/rse-engine | 組み立て。族を feature で選び、振る舞いの振り分けを持つ | rse-core・rse-blocks-* |
| crates/rse-wasm | wasm の窓口 (C ABI)。unsafe はここだけ | rse-engine |
| crates/rse-cli | native の道具 (fixture の実行器・状態表の書き出し)。serde はここだけ | rse-engine |
| packages/engine-js | wasm の読み込み・世界のハンドル・変化の窓・状態表 (TS)。窓口の決まりは docs/wasm-abi.md | rse-wasm の出力 |
| packages/board | 盤面の型と編集 (置く・消す・選ぶ)。エンジンに依らない | なし |
| packages/view-3d | 3D の描画と、押した座標の判定 (redstone-sim の packages/viewer を移す) | board の型 |
| apps/web | アプリ。機能のパッケージを組み立てるだけ | packages/* |
| conformance/fixtures | 正解の fixture。出どころを SOURCES.md に書く | — |

- 名前に minecraft・redsim・mc-tick を入れない (docs/ip-policy.md §3)。
- 生成物 (wasm・状態表の JSON) は追跡しない。ビルドの時に作る。

## 2. 依存の向き

```
rse-java → rse-core → rse-blocks-*  ┐
                    └──────────────→ rse-engine → rse-wasm → engine-js ┐
                                                 → rse-cli               ├→ apps/web
                                     board → view-3d ───────────────────┘
```

- 矢印の逆向きの依存を作らない。
- **族どうしは依存しない**。族の間のやり取り (ダストが何に繋がるか・何が信号を出すか・導体か) は、状態表の性質のビットで引く。
- 画面の機能のパッケージどうしも依存しない。共有する型は board に置く。

## 3. 振る舞いの振り分け

- rse-core が `Rules` という trait を決め、世界を `Level<R: Rules>` として持つ。rse-engine が `Rules` を 1 つだけ実装し、状態表の振る舞いの種類 (`Behavior`) を match して族の関数を呼ぶ。
- 呼び出しはジェネリクスで静的に決まる。19 §2.4 が trait の階層を避けた理由 (素子の再入と借用・間接呼び出しの無さ・match の網羅・共通の処理を set_block の 1 か所に置く) は、どれも崩れない。
- `Behavior` の種類は rse-core の状態表に全部置く (席)。rse-engine の feature で外した族の素子は、19 §2.5 の状態コード UNIMPLEMENTED を返す。黙って動かない形にはしない。
- feature は開発で族を切り離すために使う。**配る版は既定の feature (実装済みの族の全部) だけで組む** (19 §2.8 の「配る版の組み合わせを増やさない」)。

## 4. 機能の足し方

素子の族:

1. `crates/rse-blocks-<族>/` を作る。依存は rse-core だけ。
2. 族の `Behavior` が rse-core の状態表に席としてあることを確かめる (無ければ骨格の PR で足す)。
3. rse-engine に feature と match の腕を足す。
4. 族の fixture を conformance/fixtures に足し、出どころを SOURCES.md に書く。
5. 典拠のメモの節を、コメントと PR の本文に書く。

画面の機能 (3D の操作・回路ファイルの読み書き・状態の書き出し・埋め込みなど):

1. `packages/<機能>/` を作る。依存は board と engine-js まで。
2. apps/web に組み込む。機能を外す時は、apps/web から外すだけで済むようにする。

## 5. 典拠と書き方

- 挙動の典拠は redstone-sim の docs/vanilla。コメントでは「V06 §4.3」と書く (V01〜V19 は docs/vanilla/01〜19)。クラスとメソッドは、コードのコメントでは「Class.method」、docs では「Class#method」。
- Java の字面を写さない (docs/ip-policy.md)。書く役はデコンパイルの出力を読まない。
- 仮に置いた所は `UNVERIFIED` の印を付け、PR の本文に書く。
- コメントと docs は日本語で書く。

## 6. ビルドと確かめ方

| 物 | コマンド |
|---|---|
| Rust の整形・lint・テスト | `cargo fmt --all --check`・`cargo clippy --workspace --all-targets -- -D warnings`・`cargo test --workspace` |
| wasm | `cargo build -p rse-wasm --target wasm32-unknown-unknown --release` |
| wasm と状態表の JSON (engine-js が読む生成物) | `npm run build:engine`。`npm test` の前にも走る |
| JS のテスト・型 | `npm test`・`npm run typecheck` |
| 大きさの報告 | `npm run size` |

- release の設定は opt-level 3・lto・codegen-units 1・panic abort (19 §2.8)。opt-level z は使わない。
- clippy で標準の HashMap・HashSet を禁じる (clippy.toml)。反復順を Java と合わせる所は rse-java の写しを使う。
- rse-java・rse-core・族・rse-engine は no_std で、`#![forbid(unsafe_code)]` を付ける。
- 大きさ (wasm と JS) は CI で報告する。合否にしない (19 §11 の 3。サイズより正確さと実装量を優先する)。
- fixture は毎 tick、領域の全ブロックの blockstate の文字列を期待と比べる。fixture の expect をエンジンに合わせて書き換えない。

## 7. PR の決まり

- **1 本 30 ファイルまで**。超えそうなら、出す前に割る。ついでの直しは別の PR にする。
- 新しい機能は、空のコミット → draft PR → 方針のコメント → 承認 → 実装、の順で進める。
- crates/ を触る PR は、確認欄 (.github/pull_request_template.md) に答える。
