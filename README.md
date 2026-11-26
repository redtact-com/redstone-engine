# redstone-engine

レッドストーン回路のシミュレーター。実機 (Java 版) と同じ挙動を目指す Rust/WASM のエンジンと、Web のエディタ・ビューアを作り直している。

- 状態: 作り始め。まず最小限の構成で全体 (エンジン・wasm・アプリ) を通し、そのあと機能ごとに足す。
- 設計の元: redtact-com/redstone-sim の docs/research/19 (エンジン) と 20 (止めた状態の書き出しと読み込み)。挙動メモは同じ repo の docs/vanilla。
- PR は 1 本 30 ファイルまで。

非公式のツールで、Mojang / Microsoft とは無関係である。LICENSE はまだ決めていない。
