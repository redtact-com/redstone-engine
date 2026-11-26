# redstone-engine で作業するエージェントへ

レッドストーン回路のシミュレーター。実機 (Java 版 26.3) と同じ挙動を目指す Rust/WASM のエンジンと、Web のアプリを作る。
置き場と依存の向きは docs/architecture.md、著作権の決まりは docs/ip-policy.md にある。

## 必ず守ること

- **Java のコードを写さない**。典拠は redstone-sim の docs/vanilla の節 (「V06 §4.3」) と「Class.method」の名前、それに自分の言葉で書く。
- **書く役はデコンパイルの出力を読まない**。redstone-sim の tools/decompile/out と、ほかのエンジン (Nucleation・mc-tick・redsim) の clone を開かない。メモで足りなければ、実装を止めて redstone-sim に「メモへの質問」の issue を立てる。
- **PR は 1 本 30 ファイルまで**。新しい機能は draft PR で方針の承認をもらってから作る。
- fixture の expect をエンジンに合わせて書き換えない。合わない時は、エンジンかメモを疑う。
- テクスチャ・モデル・第三者のリソースパックを同梱しない。
- 生成物 (wasm・状態表の JSON・target/・dist/) と手元の絶対パスをコミットしない。

## Rust

- 標準の HashMap・HashSet を使わない (clippy.toml)。
- unsafe は rse-wasm だけ。ほかの crate は `#![forbid(unsafe_code)]`。
- 素子の振る舞いは `Behavior` の match で振り分ける。Java のクラスの階層を trait で写さない。
- 族の crate はほかの族に依存しない。族の間のやり取りは状態表の性質のビットで引く。

## 確かめるコマンド

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p rse-wasm --target wasm32-unknown-unknown --release
npm test
npm run typecheck
```

## 書き方

- コメント・docs・コミットのメッセージ・PR の本文は日本語で書く。
- 仮に置いた所は `UNVERIFIED` の印を付け、PR の本文に書く。
