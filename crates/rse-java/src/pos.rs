//! 位置と、Java 側の数値の作り方。
//!
//! ハッシュ値は HashSet の反復順 (V18 の R1・R2) を決めるので、32 ビットのあふれまで合わせる。

use crate::dir::Dir;
use crate::hash_set::JavaHash;
use core::cmp::Ordering;

/// ブロックの位置。i32 の絶対座標 (19 §3.2)。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

const X_BITS: u32 = 26;
const Z_BITS: u32 = 26;
const Y_BITS: u32 = 12;
const X_SHIFT: u32 = Y_BITS + Z_BITS; // 38
const Z_SHIFT: u32 = Y_BITS; // 12

impl Pos {
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }

    /// 1 マス隣。あふれは切り捨てる。
    #[inline]
    pub const fn offset(self, dir: Dir) -> Pos {
        let (dx, dy, dz) = dir.offset();
        Pos {
            x: self.x.wrapping_add(dx),
            y: self.y.wrapping_add(dy),
            z: self.z.wrapping_add(dz),
        }
    }

    /// 位置のハッシュ値。「(y + z × 31) × 31 + x」を 32 ビットの符号つき整数で計算し、
    /// あふれは切り捨てる (V18 §3.1。Vec3i.hashCode)。
    #[inline]
    pub const fn java_hash(self) -> i32 {
        self.y
            .wrapping_add(self.z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// 位置の比較。y → z → x の順 (V18 §3.1。Vec3i.compareTo)。
    /// `Ord` にはしない。エンジンの内部の表の鍵の順と取り違えないためである。
    #[inline]
    pub fn java_cmp(self, other: Pos) -> Ordering {
        self.y
            .cmp(&other.y)
            .then(self.z.cmp(&other.z))
            .then(self.x.cmp(&other.x))
    }

    /// 64 ビット表現。上位から x 26 ビット・z 26 ビット・y 12 ビット。
    /// 各成分は下位のビットだけを取る (V18 §3.1。BlockPos.asLong)。
    #[inline]
    pub const fn as_long(self) -> i64 {
        let x = (self.x as i64 as u64) & ((1 << X_BITS) - 1);
        let z = (self.z as i64 as u64) & ((1 << Z_BITS) - 1);
        let y = (self.y as i64 as u64) & ((1 << Y_BITS) - 1);
        ((x << X_SHIFT) | (z << Z_SHIFT) | y) as i64
    }

    /// 64 ビット表現から戻す。左へずらしてから算術右シフトで戻し、符号を復元する (V18 §3.1)。
    #[inline]
    pub const fn from_long(v: i64) -> Pos {
        Pos {
            x: (v << (64 - X_SHIFT - X_BITS) >> (64 - X_BITS)) as i32,
            y: (v << (64 - Y_BITS) >> (64 - Y_BITS)) as i32,
            z: (v << (64 - Z_SHIFT - Z_BITS) >> (64 - Z_BITS)) as i32,
        }
    }

    /// チャンクの座標 (x, z)。16 で割って切り捨てる (負の側も下へ)。
    #[inline]
    pub const fn chunk(self) -> (i32, i32) {
        (self.x >> 4, self.z >> 4)
    }

    /// チャンクのキー。下位 32 ビットが x、上位 32 ビットが z (V18 §3.1。ChunkPos.pack)。
    #[inline]
    pub const fn chunk_key(self) -> i64 {
        let (cx, cz) = self.chunk();
        ((cx as u32 as u64) | ((cz as u32 as u64) << 32)) as i64
    }
}

impl JavaHash for Pos {
    #[inline]
    fn java_hash(&self) -> i32 {
        Pos::java_hash(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // V06 §4.12.2 の表の h0 の列。
    #[test]
    fn hash_matches_notes() {
        assert_eq!(Pos::new(0, 1, 0).java_hash(), 31);
        assert_eq!(Pos::new(1, 1, 0).java_hash(), 32);
        assert_eq!(Pos::new(2, 1, 0).java_hash(), 33);
        assert_eq!(Pos::new(0, 0, 0).java_hash(), 0);
        assert_eq!(Pos::new(5, 64, 5).java_hash(), 6794);
        assert_eq!(Pos::new(-3, 64, -7).java_hash(), -4746);
        assert_eq!(Pos::new(100, 70, 200).java_hash(), 194_470);
    }

    // 展開した形「961 × z + 31 × y + x」と同じで、あふれても落ちない (V18 §3.1)。
    #[test]
    fn hash_wraps() {
        let p = Pos::new(i32::MAX, i32::MAX, i32::MAX);
        let expect = 961i32
            .wrapping_mul(p.z)
            .wrapping_add(31i32.wrapping_mul(p.y))
            .wrapping_add(p.x);
        assert_eq!(p.java_hash(), expect);
    }

    #[test]
    fn long_round_trip_keeps_sign() {
        for p in [
            Pos::new(0, 0, 0),
            Pos::new(-1, -1, -1),
            Pos::new(-2, -60, -3),
            Pos::new(1000, 64, -2000),
            Pos::new(33_554_431, 2047, 33_554_431),
            Pos::new(-33_554_432, -2048, -33_554_432),
        ] {
            assert_eq!(Pos::from_long(p.as_long()), p);
        }
        // y は下位 12 ビット、z はその上の 26 ビット、x は最上位の 26 ビット。
        assert_eq!(Pos::new(0, 1, 0).as_long(), 1);
        assert_eq!(Pos::new(0, 0, 1).as_long(), 1 << 12);
        assert_eq!(Pos::new(1, 0, 0).as_long(), 1 << 38);
    }

    #[test]
    fn chunk_key_packs_x_low_z_high() {
        assert_eq!(Pos::new(16, 0, 32).chunk(), (1, 2));
        assert_eq!(Pos::new(-1, 0, -17).chunk(), (-1, -2));
        assert_eq!(Pos::new(16, 0, 32).chunk_key(), 1 | (2 << 32));
        assert_eq!(Pos::new(-1, 0, 0).chunk_key(), 0xFFFF_FFFF);
    }

    #[test]
    fn java_cmp_is_y_then_z_then_x() {
        use core::cmp::Ordering::*;
        assert_eq!(Pos::new(9, 0, 9).java_cmp(Pos::new(0, 1, 0)), Less);
        assert_eq!(Pos::new(9, 1, 0).java_cmp(Pos::new(0, 1, 1)), Less);
        assert_eq!(Pos::new(1, 1, 1).java_cmp(Pos::new(0, 1, 1)), Greater);
        assert_eq!(Pos::new(1, 1, 1).java_cmp(Pos::new(1, 1, 1)), Equal);
    }

    #[test]
    fn offset_moves_one_cell() {
        let p = Pos::new(1, 2, 3);
        assert_eq!(p.offset(Dir::West), Pos::new(0, 2, 3));
        assert_eq!(p.offset(Dir::Up), Pos::new(1, 3, 3));
        assert_eq!(p.offset(Dir::South), Pos::new(1, 2, 4));
    }
}
