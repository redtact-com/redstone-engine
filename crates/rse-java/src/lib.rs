//! Java の意味論の写し。
//!
//! 実機の結果が Java の標準ライブラリの振る舞い (HashSet の反復順・数値の丸めなど) に
//! 左右される所を、ここに写す (docs/architecture.md)。no_std で、外部の依存を持たない。
//!
//! | モジュール | 中身 | 典拠 |
//! |---|---|---|
//! | [`dir`] | 方向と、用途ごとの並び | V04 §2.3・V06 §3.6 |
//! | [`pos`] | 位置・ハッシュ値・64 ビットの詰め方・チャンクのキー | V18 §3.1 |
//! | [`hash_set`] | HashSet の反復順の写し | V18 §4.2 (R1 が使う) |
//!
//! 乱数・開番地の写し・正弦表は、使う素子が入る PR で足す。
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod dir;
pub mod hash_set;
pub mod pos;

pub use dir::{Axis, Dir};
pub use hash_set::{JavaHash, JavaHashSet, TreeBin};
pub use pos::Pos;
