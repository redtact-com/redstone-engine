//! java.util.HashSet の反復順の写し (V18 §4.2)。
//!
//! 実機は、ダストが近隣更新を配る 7 つの起点を HashSet に入れ、その反復順に配る (V18 の R1)。
//! 反復順は「入れ物の番号の昇順。同じ入れ物の中は入った順」で、番号は位置のハッシュ値から決まる。
//! 同じ回路でも置く場所で順が変わるので、近道の式ではなく、入れ物の動きをそのまま持つ。
//!
//! 木構造になった入れ物は実装しない。条件に達したら [`TreeBin`] を返す (19 §2.5 の TREE_BIN)。

use alloc::vec::Vec;

/// Java 側のハッシュ値 (32 ビットの符号つき整数) を返す。
pub trait JavaHash {
    fn java_hash(&self) -> i32;
}

/// 入れ物が木構造になる条件 (入れ物の数が 64 以上の表で、1 つの入れ物に 9 個) に達した (V18 §4.2)。
/// 木の中の順は識別ハッシュに左右されるので写せない。要素は入れずに返す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TreeBin;

/// 最初の入れ物の数 (V18 §4.2)。
const INITIAL_BUCKETS: usize = 16;
/// 1 つの入れ物がこの個数になったら、拡張か木構造化を試みる (V18 §4.2)。
const TREEIFY_AT: usize = 9;
/// 入れ物の数がこれ未満なら、木にせず表を 2 倍にする (V18 §4.2)。
const MIN_TREEIFY_BUCKETS: usize = 64;

/// HashSet の写し。反復順が Java と同じになる。
#[derive(Clone, Debug, Default)]
pub struct JavaHashSet<K> {
    /// 入れ物の配列。空の表は長さ 0 で、最初に入れる時に作る。
    buckets: Vec<Vec<K>>,
    len: usize,
    /// 要素数がこれを超えたら拡張する (入れ物の数の 3/4)。
    threshold: usize,
}

/// 撹拌。ハッシュ値と、それを符号なしで 16 ビット右へずらした値との排他的論理和 (V18 §4.2)。
#[inline]
fn spread(h: i32) -> u32 {
    let h = h as u32;
    h ^ (h >> 16)
}

impl<K: JavaHash + Eq> JavaHashSet<K> {
    /// 空の集合。入れ物は最初に入れる時に 16 個で作る。
    pub const fn new() -> Self {
        JavaHashSet {
            buckets: Vec::new(),
            len: 0,
            threshold: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 今の入れ物の数 (未作成なら 0)。
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    #[inline]
    fn index_of(&self, key: &K) -> usize {
        spread(key.java_hash()) as usize & (self.buckets.len() - 1)
    }

    pub fn contains(&self, key: &K) -> bool {
        !self.buckets.is_empty() && self.buckets[self.index_of(key)].contains(key)
    }

    /// 入れる。新しく入ったら `Ok(true)`、既にあれば `Ok(false)` (位置は変えない)。
    ///
    /// 手順は V18 §4.2 の「入れる」のとおり:
    /// 入れ物の末尾につなぐ → その入れ物が 9 個以上なら拡張 (入れ物の数が 64 未満のとき) →
    /// 要素数がしきいを超えたら拡張。
    pub fn insert(&mut self, key: K) -> Result<bool, TreeBin> {
        if self.buckets.is_empty() {
            self.buckets.resize_with(INITIAL_BUCKETS, Vec::new);
            self.threshold = INITIAL_BUCKETS / 4 * 3;
        }
        let i = self.index_of(&key);
        if self.buckets[i].contains(&key) {
            return Ok(false);
        }
        if self.buckets[i].len() + 1 >= TREEIFY_AT && self.buckets.len() >= MIN_TREEIFY_BUCKETS {
            return Err(TreeBin);
        }
        self.buckets[i].push(key);
        if self.buckets[i].len() >= TREEIFY_AT {
            self.grow();
        }
        self.len += 1;
        if self.len > self.threshold {
            self.grow();
        }
        Ok(true)
    }

    /// 消す。つながりから外すだけで、表は縮まない。残りの相対順は変わらない (V18 §4.2)。
    pub fn remove(&mut self, key: &K) -> bool {
        if self.buckets.is_empty() {
            return false;
        }
        let i = self.index_of(key);
        match self.buckets[i].iter().position(|k| k == key) {
            Some(at) => {
                self.buckets[i].remove(at);
                self.len -= 1;
                true
            }
            None => false,
        }
    }

    /// 表を 2 倍にする。古い入れ物 j の中身は、順を保ったまま j と「j + 古い入れ物の数」に分かれる (V18 §4.2)。
    fn grow(&mut self) {
        let old_n = self.buckets.len();
        let mut next: Vec<Vec<K>> = Vec::new();
        next.resize_with(old_n * 2, Vec::new);
        for (j, bucket) in core::mem::take(&mut self.buckets).into_iter().enumerate() {
            for key in bucket {
                let high = spread(key.java_hash()) as usize & old_n != 0;
                next[if high { j + old_n } else { j }].push(key);
            }
        }
        self.buckets = next;
        self.threshold *= 2;
    }

    /// 反復。入れ物を番号の昇順に見て、入れ物の中は入った順 (V18 §4.2)。
    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.buckets.iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dir::Dir::{self, *};
    use crate::pos::Pos;

    /// 位置を使わず、ハッシュ値を直に指定する鍵。
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct H(i32);
    impl JavaHash for H {
        fn java_hash(&self) -> i32 {
            self.0
        }
    }

    /// R1 の手順 (V18 §4.5): 自分 → 宣言順の 6 つの隣、の順に入れて反復する。
    /// 戻り値は「自分なら None、隣ならその方向」。
    fn r1_order(p: Pos) -> Vec<Option<Dir>> {
        let mut set = JavaHashSet::new();
        set.insert(p).unwrap();
        for d in Dir::ALL {
            set.insert(p.offset(d)).unwrap();
        }
        set.iter()
            .map(|q| Dir::ALL.into_iter().find(|d| p.offset(*d) == *q))
            .collect()
    }

    const P: Option<Dir> = None;
    fn s(d: Dir) -> Option<Dir> {
        Some(d)
    }

    // V06 §4.12.2 の表 (原点の近くは組が崩れる例) と、V18 §4.2 の「R1 の実例」[ライブラリ実測]。
    #[test]
    fn r1_order_matches_notes() {
        let cases: [((i32, i32, i32), [Option<Dir>; 7]); 9] = [
            (
                (0, 1, 0),
                [s(Down), s(South), s(East), s(North), s(Up), s(West), P],
            ),
            (
                (1, 1, 0),
                [P, s(North), s(Down), s(South), s(East), s(Up), s(West)],
            ),
            (
                (2, 1, 0),
                [s(Up), s(West), P, s(Down), s(South), s(East), s(North)],
            ),
            (
                (0, 0, 0),
                [P, s(North), s(West), s(South), s(East), s(Down), s(Up)],
            ),
            (
                (5, 64, 5),
                [s(Up), s(North), s(West), P, s(Down), s(South), s(East)],
            ),
            (
                (-3, 64, -7),
                [s(Down), s(South), s(East), P, s(Up), s(North), s(West)],
            ),
            (
                (100, 70, 200),
                [P, s(Down), s(South), s(East), s(Up), s(North), s(West)],
            ),
            (
                (-2, -60, -3),
                [s(Down), s(South), s(East), P, s(Up), s(North), s(West)],
            ),
            (
                (1000, 64, -2000),
                [s(Up), s(North), s(West), P, s(Down), s(South), s(East)],
            ),
        ];
        for ((x, y, z), expect) in cases {
            assert_eq!(r1_order(Pos::new(x, y, z)), expect, "p = ({x}, {y}, {z})");
        }
    }

    #[test]
    fn duplicate_keeps_position() {
        let mut set = JavaHashSet::new();
        assert_eq!(set.insert(H(1)), Ok(true));
        assert_eq!(set.insert(H(17)), Ok(true)); // 入れ物 1 に並ぶ
        assert_eq!(set.insert(H(1)), Ok(false));
        assert_eq!(set.len(), 2);
        assert_eq!(set.iter().copied().collect::<Vec<_>>(), [H(1), H(17)]);
    }

    #[test]
    fn remove_keeps_relative_order_and_size() {
        let mut set = JavaHashSet::new();
        for h in [1, 17, 33, 2] {
            set.insert(H(h)).unwrap();
        }
        assert!(set.remove(&H(17)));
        assert!(!set.remove(&H(17)));
        assert!(!set.contains(&H(17)));
        assert_eq!(set.bucket_count(), 16);
        assert_eq!(
            set.iter().copied().collect::<Vec<_>>(),
            [H(1), H(33), H(2)]
        );
    }

    // 13 個目で 16 → 32 に拡張する (V18 §4.2 の「入れる」の 6)。
    #[test]
    fn grows_past_threshold() {
        let mut set = JavaHashSet::new();
        for h in 0..12 {
            set.insert(H(h)).unwrap();
        }
        assert_eq!(set.bucket_count(), 16);
        set.insert(H(12)).unwrap();
        assert_eq!(set.bucket_count(), 32);
        // 25 個目で 64。
        for h in 13..25 {
            set.insert(H(h)).unwrap();
        }
        assert_eq!(set.bucket_count(), 64);
    }

    // 拡張は、古い入れ物の中の順を保ったまま 2 本に分ける (V18 §4.2 の「拡張」)。
    #[test]
    fn split_keeps_order_within_bucket() {
        let mut set = JavaHashSet::new();
        // 入れ物 0 に 0・16・32・48 を入れる。16 と 48 は「古い入れ物の数のビット」が立つ。
        for h in [0, 16, 32, 48] {
            set.insert(H(h)).unwrap();
        }
        // 残りを別々の入れ物へ入れて 13 個にする。
        for h in 1..=9 {
            set.insert(H(h)).unwrap();
        }
        assert_eq!(set.bucket_count(), 32);
        let order: Vec<i32> = set.iter().map(|k| k.0).collect();
        assert_eq!(order, [0, 32, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 48]);
    }

    // 1 つの入れ物に 9 個で、入れ物の数が 64 未満なら拡張する (V18 §4.2 の「入れる」の 5)。
    #[test]
    fn ninth_in_a_bucket_grows_table() {
        let mut set = JavaHashSet::new();
        // 下位 6 ビットが同じなので、16・32・64 個のどの表でも同じ入れ物に入る。
        for i in 0..8 {
            set.insert(H(i * 64)).unwrap();
        }
        assert_eq!(set.bucket_count(), 16);
        set.insert(H(8 * 64)).unwrap();
        assert_eq!(set.bucket_count(), 32);
        set.insert(H(9 * 64)).unwrap();
        assert_eq!(set.bucket_count(), 64);
        // 64 個の表で 11 個目を同じ入れ物へ入れると、木構造の条件に達する。入れずに返す。
        assert_eq!(set.insert(H(10 * 64)), Err(TreeBin));
        assert_eq!(set.len(), 10);
        assert!(!set.contains(&H(10 * 64)));
    }

    // 撹拌は上位 16 ビットを下位へ混ぜる (V18 §4.2 の「入れ物の番号の決め方」)。
    #[test]
    fn spread_mixes_high_bits() {
        assert_eq!(spread(0), 0);
        assert_eq!(spread(0x0001_0000), 0x0001_0001);
        assert_eq!(spread(-1), 0xFFFF_0000);
    }
}
