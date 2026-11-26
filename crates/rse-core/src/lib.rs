//! エンジンの骨格。
//!
//! 素子の族を知らない。族は rse-core の公開の関数だけを使い、振る舞いの振り分けは
//! rse-engine が [`Rules`] を実装して受け持つ (docs/architecture.md §3)。no_std + alloc。
//!
//! 順序は骨格の部品が決める。素子は「何を・どの順で頼むか」だけを書く (19 §2.4)。
//!
//! | モジュール | 中身 | 典拠 |
//! |---|---|---|
//! | [`state`]・[`table`] | StateId・BlockId・性質のビット・混合基数・縦切りの 5 種の表 | V05・19 §3.1 |
//! | [`world`] | セクションと格子・読み書き・変化の記録 | 19 §3.2・§3.8 |
//! | [`level`] | 世界 1 つ分の状態 (`Level<R: Rules>`) | docs/architecture.md §3 |
//! | [`rules`] | 振る舞いの振り分けの口 | 19 §2.4 |
//! | [`set_block`] | 書き込みの手順とフラグ | V04 §3.1・§4.1・§4.2・§4.10 |
//! | [`update`] | 近隣更新・形状更新・コンパレーター更新の深さ優先のスタック | V04 §4.3〜§4.8・19 §3.6 |
//! | [`ticks`] | 予約 tick (2 層・予約順・重複の抑止) | V02・19 §3.4 |
//! | [`events`] | ブロックイベント (挿入順の列) | V03・19 §3.5 |
//! | [`phases`] | 相の表と 1 tick・凍結 | V01 §2.3・§7.1・19 §4.1 |
//! | [`input`] | tick の外の入力と区間の印 | V01 §2.4・19 §4.1 |
//! | [`signal`] | 信号の問い合わせ (弱い・強い・導体) | V05 §2.3・§4.1 |
//! | [`status`] | 状態コード | 19 §2.5 |
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod events;
pub mod input;
pub mod level;
pub mod phases;
pub mod rules;
pub mod set_block;
pub mod signal;
pub mod state;
pub mod status;
pub mod table;
pub mod ticks;
pub mod update;
pub mod world;

#[cfg(test)]
mod testing;

pub use input::{Input, InputOutcome, InputSection};
pub use level::{Config, Level};
pub use phases::{PHASES, Phase};
pub use rse_java::{Dir, Pos};
pub use rules::Rules;
pub use set_block::UpdateFlags;
pub use state::{Behavior, BlockId, StateId};
pub use status::Status;
pub use ticks::TickPriority;
pub use update::Orientation;
pub use world::Change;
