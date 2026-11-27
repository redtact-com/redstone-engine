//! 方向と、用途ごとの並び。
//!
//! 並びは用途ごとに別の定数で持つ。1 本を使い回すと、近隣更新と形状更新の順が入れ替わる
//! (V04 §2.3 の表・V06 §3.6 の表)。

/// 軸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// 方向。番号は宣言順 (下・上・北・南・西・東) で、V04 §2.3 の Direction の宣言順と同じ。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Dir {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Dir {
    /// 宣言順: 下・上・北・南・西・東 (V04 §2.3)。
    /// 信号の問い合わせ (V05 §2.3) と、ダストが HashSet に入れる順 (V18 の R1) が使う。
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];

    /// 近隣更新 (NC) の 6 方向: 西・東・下・上・北・南 (V04 §2.3・§4.5。NeighborUpdater の UPDATE_ORDER)。
    pub const NC_ORDER: [Dir; 6] = [
        Dir::West,
        Dir::East,
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
    ];

    /// 形状更新 (SU) の 6 方向: 西・東・北・南・下・上 (V04 §2.3・§4.6。BlockBehaviour の UPDATE_SHAPE_ORDER)。
    pub const SU_ORDER: [Dir; 6] = [
        Dir::West,
        Dir::East,
        Dir::North,
        Dir::South,
        Dir::Down,
        Dir::Up,
    ];

    /// 水平の並び: 北・東・南・西 (V04 §2.3。コンパレーター更新 CU と、ダストの接続の計算が使う)。
    /// blockstate の水平の向きの値の並び (北・南・西・東。V05 §3.2) とは違う。
    pub const HORIZONTAL: [Dir; 4] = [Dir::North, Dir::East, Dir::South, Dir::West];

    /// 垂直の並び: 上・下 (V04 §2.3)。
    pub const VERTICAL: [Dir; 2] = [Dir::Up, Dir::Down];

    /// 宣言順の番号 (0〜5)。
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// 座標の差分 (x, y, z) (V04 §2.3 の表)。
    #[inline]
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Dir::Down => (0, -1, 0),
            Dir::Up => (0, 1, 0),
            Dir::North => (0, 0, -1),
            Dir::South => (0, 0, 1),
            Dir::West => (-1, 0, 0),
            Dir::East => (1, 0, 0),
        }
    }

    /// 逆の方向。
    #[inline]
    pub const fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }

    /// 軸。
    #[inline]
    pub const fn axis(self) -> Axis {
        match self {
            Dir::Down | Dir::Up => Axis::Y,
            Dir::North | Dir::South => Axis::Z,
            Dir::West | Dir::East => Axis::X,
        }
    }

    /// 水平の方向か。
    #[inline]
    pub const fn is_horizontal(self) -> bool {
        !matches!(self, Dir::Down | Dir::Up)
    }
}

#[cfg(test)]
mod tests {
    use super::Dir::*;
    use super::*;

    // 並びは実機の順序そのものなので、表を値で固定する (V04 §7.2 の 2〜4)。
    #[test]
    fn tables_are_fixed() {
        assert_eq!(Dir::ALL, [Down, Up, North, South, West, East]);
        assert_eq!(Dir::NC_ORDER, [West, East, Down, Up, North, South]);
        assert_eq!(Dir::SU_ORDER, [West, East, North, South, Down, Up]);
        assert_eq!(Dir::HORIZONTAL, [North, East, South, West]);
        assert_eq!(Dir::VERTICAL, [Up, Down]);
    }

    #[test]
    fn index_matches_declaration_order() {
        for (i, d) in Dir::ALL.iter().enumerate() {
            assert_eq!(d.index(), i);
        }
    }

    #[test]
    fn offsets_and_opposites() {
        assert_eq!(Down.offset(), (0, -1, 0));
        assert_eq!(Up.offset(), (0, 1, 0));
        assert_eq!(North.offset(), (0, 0, -1));
        assert_eq!(South.offset(), (0, 0, 1));
        assert_eq!(West.offset(), (-1, 0, 0));
        assert_eq!(East.offset(), (1, 0, 0));
        for d in Dir::ALL {
            let (x, y, z) = d.offset();
            assert_eq!(d.opposite().offset(), (-x, -y, -z));
            assert_eq!(d.opposite().opposite(), d);
            assert_eq!(d.axis(), d.opposite().axis());
            assert_eq!(d.is_horizontal(), d.axis() != Axis::Y);
        }
    }
}
