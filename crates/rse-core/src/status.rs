//! 状態コード (19 §2.5 の表)。
//!
//! エンジンは、実機と違う結果になりうる所を黙って通らない。ビットの集合で呼び出し元へ返し、
//! 文は呼び出し元 (JS 側) の表で引く。

use core::ops::{BitOr, BitOrAssign};

/// 状態コードのビットの集合。0 は「何も無い」。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Status(pub u32);

impl Status {
    /// 何も起きていない。
    pub const OK: Status = Status(0);
    /// 近隣更新の連鎖が上限に達し、新しい分を捨てた (V04 §4.3・V18 §7.4)。
    pub const CHAIN_LIMIT: Status = Status(1 << 0);
    /// 予約 tick が 1 tick の上限に達し、持ち越した (V02 §4.3)。
    pub const TICK_CARRYOVER: Status = Status(1 << 1);
    /// 別のチャンクの予約どうしが (優先度, 予約順) で同点になった。順が一意でない (V02 §5.5・V18 の R4)。
    pub const LOADED_ORDER_TIE: Status = Status(1 << 2);
    /// HashMap の写しが木構造の条件に達した (V18 §4.2。実装していない)。
    pub const TREE_BIN: Status = Status(1 << 3);
    /// 未実装の経路を通った。
    pub const UNIMPLEMENTED: Status = Status(1 << 4);
    /// 安全弁が働き、実機と違う結果になった (V03 §7.2)。
    pub const SAFETY: Status = Status(1 << 5);

    /// 一覧 (ビットの番号の順)。JS 側の表と突き合わせるために使う。
    pub const ALL: [(Status, &'static str); 6] = [
        (Status::CHAIN_LIMIT, "CHAIN_LIMIT"),
        (Status::TICK_CARRYOVER, "TICK_CARRYOVER"),
        (Status::LOADED_ORDER_TIE, "LOADED_ORDER_TIE"),
        (Status::TREE_BIN, "TREE_BIN"),
        (Status::UNIMPLEMENTED, "UNIMPLEMENTED"),
        (Status::SAFETY, "SAFETY"),
    ];

    #[inline]
    pub const fn is_ok(self) -> bool {
        self.0 == 0
    }

    /// `other` のビットを全部含むか。
    #[inline]
    pub const fn contains(self, other: Status) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Status {
    type Output = Status;
    #[inline]
    fn bitor(self, rhs: Status) -> Status {
        Status(self.0 | rhs.0)
    }
}

impl BitOrAssign for Status {
    #[inline]
    fn bitor_assign(&mut self, rhs: Status) {
        self.0 |= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ビットの割り当ては wasm の境界を越えるので、値で固定する。
    #[test]
    fn bits_are_fixed() {
        let bits: Vec<u32> = Status::ALL.iter().map(|(s, _)| s.0).collect();
        assert_eq!(bits, [1, 2, 4, 8, 16, 32]);
        let names: Vec<&str> = Status::ALL.iter().map(|(_, n)| *n).collect();
        assert_eq!(
            names,
            [
                "CHAIN_LIMIT",
                "TICK_CARRYOVER",
                "LOADED_ORDER_TIE",
                "TREE_BIN",
                "UNIMPLEMENTED",
                "SAFETY"
            ]
        );
    }

    #[test]
    fn set_operations() {
        let mut s = Status::OK;
        assert!(s.is_ok());
        s |= Status::CHAIN_LIMIT;
        s |= Status::SAFETY;
        assert!(s.contains(Status::CHAIN_LIMIT));
        assert!(s.contains(Status::CHAIN_LIMIT | Status::SAFETY));
        assert!(!s.contains(Status::UNIMPLEMENTED));
        assert!(!s.is_ok());
    }
}
