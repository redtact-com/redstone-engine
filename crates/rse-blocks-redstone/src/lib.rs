//! 素子の族: レバー・ダスト・ランプ。
//!
//! 骨格 (rse-core) の公開の関数だけを使う。ほかの族には依存しない。
//! 族の間のやり取り (ダストが何に繋がるか・何が信号を出すか・導体か・面が支えるか) は、
//! 状態表の性質のビットで引く (docs/architecture.md §2)。
//!
//! ここに置くのは素子ごとの関数だけで、振り分けは持たない。フックのどれをどの素子へ渡すかは、
//! rse-engine が `Behavior` の match で決める (docs/architecture.md §3)。
//!
//! | モジュール | 素子 | 典拠 |
//! |---|---|---|
//! | [`lever`] | レバー (切り替え・信号・取り付け・設置) | V09 §3.1・§4.1 |
//! | [`wire`] | ダスト (接続の形・信号の出力・既定の評価器・設置と除去・支え) | V06 |
//! | [`lamp`] | ランプ (点灯はその場・消灯は 4gt の予約 tick) | V11 §4.1・§5.1 |
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
// doctest には crate の forbid が効かないので、doctest にも付ける (docs/wasm-abi.md §5)。今は doctest が無い。
#![doc(test(attr(forbid(unsafe_code))))]

pub mod lamp;
pub mod lever;
pub mod wire;

#[cfg(test)]
mod testing;

use rse_core::Rules;

/// 族が世界ごとに持つ状態。`Rules` を実装する型 (rse-engine) が 1 つ持つ。
#[derive(Clone, Debug)]
pub struct RedstoneState {
    /// ダストが信号を出すか。ふだんは真で、ダストが周りのブロックの信号を読む間だけ偽にする
    /// (V06 §3.3・§4.9。RedstoneWireBlock の shouldSignal)。
    /// 偽の間は、ダストの弱い信号・強い信号・信号源の印がどれも消える (V06 §4.8)。
    pub(crate) wire_gives_signal: bool,
}

impl RedstoneState {
    pub const fn new() -> RedstoneState {
        RedstoneState {
            wire_gives_signal: true,
        }
    }

    /// ダストが信号を出しているか。信号源の印 (Rules.is_signal_source) の答えに使う (V05 §7.2)。
    #[inline]
    pub fn wire_gives_signal(&self) -> bool {
        self.wire_gives_signal
    }
}

impl Default for RedstoneState {
    fn default() -> RedstoneState {
        RedstoneState::new()
    }
}

/// この族の状態を持つ `Rules`。ダストの関数が、評価中の印を読み書きするのに使う。
pub trait RedstoneRules: Rules {
    fn redstone(&self) -> &RedstoneState;
    fn redstone_mut(&mut self) -> &mut RedstoneState;
}
