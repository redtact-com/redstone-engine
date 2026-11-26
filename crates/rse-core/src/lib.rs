//! エンジンの骨格。
//!
//! 素子の族を知らない。族は rse-core の公開の関数だけを使い、振る舞いの振り分けは
//! rse-engine が `Rules` を実装して受け持つ (docs/architecture.md)。no_std + alloc。
//! 縦切りで足す物: 状態表 (最小)・世界・書き込みと近隣更新・予約 tick・ブロックイベントの枠・
//! 相・入力・信号の問い合わせ・変化の記録・状態コード (#1)。
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
