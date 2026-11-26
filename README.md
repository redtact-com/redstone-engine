# redstone-engine

レッドストーン回路のシミュレーター。実機 (Java 版 26.3) と同じ挙動を目指す Rust/WASM のエンジンと、Web のエディタ・ビューアを作り直している。

非公式のツールで、Mojang / Microsoft とは無関係である。LICENSE はまだ決めていない。

## 状態

作り始め。まず最小限の構成で全体を通す (#1)。レバー・ダスト・ランプが Rust で動き、wasm を通して Web のアプリで置いて動かせるところまでが最初の到達点である。そのあと機能ごとに足す。

## 置き場

| 置き場 | 中身 |
|---|---|
| crates/ | Rust のエンジン (rse-java・rse-core・rse-blocks-*・rse-engine・rse-wasm・rse-cli) |
| packages/ | TS のパッケージ (engine-js・board・view-3d) |
| apps/web | Web のアプリ |
| conformance/ | 正解の fixture |
| docs/ | 作りの決まり (architecture.md)・著作権の運用 (ip-policy.md) |

## 設計の元

- redtact-com/redstone-sim の docs/research/19 (Rust/WASM のエンジン) と 20 (止めた状態の書き出しと読み込み)
- 挙動の典拠は同じ repo の docs/vanilla (26.3 のゲームコードを読んで自分の言葉で書いたメモ)

## 確かめる

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p rse-wasm --target wasm32-unknown-unknown --release
npm ci
npm test
npm run typecheck
```

## 決まり

- PR は 1 本 30 ファイルまで。新しい機能は draft PR で方針の承認をもらってから作る。
- Java のコードを写さない。典拠はメモの節で書く (docs/ip-policy.md)。
