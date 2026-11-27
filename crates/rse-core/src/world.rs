//! 世界: セクションと格子・読み書き・変化の記録 (19 §3.2・§3.8)。
//!
//! - セクションは 16³ の StateId の密な配列。参照は配列 1 回で済む。
//! - セクションの場所は、世界の広がりを覆う密な格子で持つ。外へ広がれば作り直す。
//!   ハッシュを引かないので、順が決まる。
//! - 座標は i32 の絶対座標。ダストの起点の順 (V18 の R1) が絶対座標で変わるので、原点を動かさない。
//! - 高さの外は空気として読む (V05 §2.3 の 1 の void_air の代わり。信号 0・非導体で、性質は同じ)。
//!
//! セクションの中の近道 (添字に ±1・±16・±256 を足す) と、ランダム tick の対象数は、後の PR で足す。

use crate::state::StateId;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use rse_java::Pos;

/// 正味の変化 1 件 (19 §3.8)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Change {
    pub pos: Pos,
    pub state: StateId,
}

const SECTION_CELLS: usize = 16 * 16 * 16;

struct Section {
    cells: Box<[StateId]>,
    /// マスごとの「書き出しの後に書き換えた」の印。
    dirty: [u64; SECTION_CELLS / 64],
}

impl Section {
    fn new() -> Box<Section> {
        Box::new(Section {
            cells: vec![StateId::AIR; SECTION_CELLS].into_boxed_slice(),
            dirty: [0; SECTION_CELLS / 64],
        })
    }
}

/// セクションの中の添字。y が最上位で、x が最下位。
#[inline]
fn cell_index(pos: Pos) -> usize {
    (((pos.y & 15) as usize) << 8) | (((pos.z & 15) as usize) << 4) | (pos.x & 15) as usize
}

/// 格子の 1 辺の上限 (セクションの数)。エンジンの決めた枠で、実機には無い。
/// 離れた 2 点へ書いて格子が膨らみ、メモリを使い切るのを防ぐ。
const MAX_GRID_SPAN: i64 = 256;

/// セクションの座標 (x・y・z)。
type Section3 = (i32, i32, i32);

fn section_of(pos: Pos) -> Section3 {
    (pos.x >> 4, pos.y >> 4, pos.z >> 4)
}

/// 範囲 (両端を含む) に、セクションを 1 つ足した箱。
fn with_section(bounds: Option<(Section3, Section3)>, s: Section3) -> (Section3, Section3) {
    match bounds {
        None => (s, s),
        Some((lo, hi)) => (
            (lo.0.min(s.0), lo.1.min(s.1), lo.2.min(s.2)),
            (hi.0.max(s.0), hi.1.max(s.1), hi.2.max(s.2)),
        ),
    }
}

/// 箱の各辺が枠に収まるか。
fn fits(lo: Section3, hi: Section3) -> bool {
    let span = |a: i32, b: i32| b as i64 - a as i64 + 1;
    span(lo.0, hi.0) <= MAX_GRID_SPAN
        && span(lo.1, hi.1) <= MAX_GRID_SPAN
        && span(lo.2, hi.2) <= MAX_GRID_SPAN
}

/// 世界のブロックの格子。
pub struct World {
    min_y: i32,
    max_y: i32,
    /// 格子の原点 (セクションの座標 x・y・z)。
    origin: (i32, i32, i32),
    /// 格子の大きさ (セクションの数 x・y・z)。
    size: (usize, usize, usize),
    sections: Vec<Option<Box<Section>>>,
    /// 書き出しの後に初めて書いたマスと、その前の状態 (19 §3.8)。
    touched: Vec<(Pos, StateId)>,
    /// 書き出しの列。使い回す。
    out: Vec<Change>,
}

impl World {
    /// `min_y` 以上・`min_y + height` 未満が、書ける高さになる。
    pub fn new(min_y: i32, height: i32) -> World {
        World {
            min_y,
            max_y: min_y.saturating_add(height.max(0)),
            origin: (0, 0, 0),
            size: (0, 0, 0),
            sections: Vec::new(),
            touched: Vec::new(),
            out: Vec::new(),
        }
    }

    /// 書ける高さの内か。
    #[inline]
    pub fn in_bounds(&self, pos: Pos) -> bool {
        pos.y >= self.min_y && pos.y < self.max_y
    }

    #[inline]
    fn slot(&self, pos: Pos) -> Option<usize> {
        let sx = (pos.x >> 4).wrapping_sub(self.origin.0) as u32 as usize;
        let sy = (pos.y >> 4).wrapping_sub(self.origin.1) as u32 as usize;
        let sz = (pos.z >> 4).wrapping_sub(self.origin.2) as u32 as usize;
        if sx < self.size.0 && sy < self.size.1 && sz < self.size.2 {
            Some((sy * self.size.2 + sz) * self.size.0 + sx)
        } else {
            None
        }
    }

    /// 読む。高さの外と、まだ何も置いていない所は空気。
    #[inline]
    pub fn get(&self, pos: Pos) -> StateId {
        match self.slot(pos).and_then(|i| self.sections[i].as_ref()) {
            Some(section) => section.cells[cell_index(pos)],
            None => StateId::AIR,
        }
    }

    /// 今の格子が覆うセクションの範囲 (両端を含む)。格子がまだ無ければ `None`。
    fn grid_bounds(&self) -> Option<(Section3, Section3)> {
        if self.sections.is_empty() {
            return None;
        }
        let o = self.origin;
        let end = (
            o.0 + self.size.0 as i32 - 1,
            o.1 + self.size.1 as i32 - 1,
            o.2 + self.size.2 as i32 - 1,
        );
        Some((o, end))
    }

    /// 書きたいマスを全部書けるか。高さの内で、空気でないマスのために格子を広げても枠
    /// (1 辺 256 セクション) に収まるなら真。何も書かず、格子も広げない。
    ///
    /// 格子は、今の範囲と書くマスのセクションを合わせた箱へ広がる (`grow_to`)。途中の箱は最後の箱に含まれるので、
    /// 最後の箱が枠に収まれば、どの順に書いても全部書ける。空気は、何も無い所へ書いても格子を広げないので数えない。
    /// 読み込みを全部か無しかにする (1 つも書かずに断る) ために、書く前に確かめる。
    pub fn can_set_all(&self, cells: impl IntoIterator<Item = (Pos, StateId)>) -> bool {
        let mut bounds = self.grid_bounds();
        for (pos, state) in cells {
            if !self.in_bounds(pos) {
                return false;
            }
            if state != StateId::AIR {
                bounds = Some(with_section(bounds, section_of(pos)));
            }
        }
        bounds.is_none_or(|(lo, hi)| fits(lo, hi))
    }

    /// 格子を、`pos` のセクションを含む広さへ作り直す。枠を超えるなら偽。
    fn grow_to(&mut self, pos: Pos) -> bool {
        let (lo, hi) = with_section(self.grid_bounds(), section_of(pos));
        if !fits(lo, hi) {
            return false;
        }
        let span = |a: i32, b: i32| (b as i64 - a as i64 + 1) as usize;
        let size = (span(lo.0, hi.0), span(lo.1, hi.1), span(lo.2, hi.2));
        let mut next: Vec<Option<Box<Section>>> = Vec::new();
        next.resize_with(size.0 * size.1 * size.2, || None);
        let old = core::mem::take(&mut self.sections);
        let (ox, oy, oz) = self.origin;
        let (sx, _, sz) = self.size;
        for (i, section) in old.into_iter().enumerate() {
            if section.is_none() {
                continue;
            }
            let x = (i % sx) as i32 + ox - lo.0;
            let z = (i / sx % sz) as i32 + oz - lo.2;
            let y = (i / sx / sz) as i32 + oy - lo.1;
            next[(y as usize * size.2 + z as usize) * size.0 + x as usize] = section;
        }
        self.sections = next;
        self.origin = lo;
        self.size = size;
        true
    }

    /// 書く。前の状態を返す。高さの外と、格子の枠を超える所へは書けず、`None` を返す。
    /// 更新は出さない。変化の記録だけを付ける (19 §3.8)。
    pub fn set(&mut self, pos: Pos, state: StateId) -> Option<StateId> {
        if !self.in_bounds(pos) {
            return None;
        }
        let slot = match self.slot(pos) {
            Some(slot) => slot,
            None => {
                // 何も無い所へ空気を書くだけなら、格子を広げない (V04 §4.2 の手順 1)。
                if state == StateId::AIR {
                    return Some(StateId::AIR);
                }
                if !self.grow_to(pos) {
                    return None;
                }
                self.slot(pos)?
            }
        };
        if self.sections[slot].is_none() {
            if state == StateId::AIR {
                return Some(StateId::AIR);
            }
            self.sections[slot] = Some(Section::new());
        }
        let section = self.sections[slot].as_mut()?;
        let cell = cell_index(pos);
        let old = section.cells[cell];
        if old != state {
            section.cells[cell] = state;
            let (word, bit) = (cell / 64, 1u64 << (cell % 64));
            if section.dirty[word] & bit == 0 {
                section.dirty[word] |= bit;
                self.touched.push((pos, old));
            }
        }
        Some(old)
    }

    /// 前の書き出しの後の、正味の変化を返す。書き換えて元に戻したマスは出ない (19 §3.8)。
    /// 並びは、マスを最初に書いた順。
    pub fn drain_changes(&mut self) -> &[Change] {
        self.out.clear();
        let touched = core::mem::take(&mut self.touched);
        for (pos, before) in &touched {
            let Some(section) = self.slot(*pos).and_then(|i| self.sections[i].as_mut()) else {
                continue;
            };
            let cell = cell_index(*pos);
            section.dirty[cell / 64] &= !(1u64 << (cell % 64));
            let now = section.cells[cell];
            if now != *before {
                self.out.push(Change {
                    pos: *pos,
                    state: now,
                });
            }
        }
        // 割り当てを使い回す。
        self.touched = touched;
        self.touched.clear();
        &self.out
    }

    /// 書き出しを待っているマスの数 (正味で変わっていない分も数える)。
    pub fn pending_changes(&self) -> usize {
        self.touched.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::block;

    fn world() -> World {
        World::new(-64, 384)
    }

    #[test]
    fn reads_air_where_nothing_was_placed() {
        let w = world();
        assert_eq!(w.get(Pos::new(0, 0, 0)), StateId::AIR);
        assert_eq!(w.get(Pos::new(1_000_000, 500, -1_000_000)), StateId::AIR);
    }

    #[test]
    fn set_and_get_across_sections_and_negative_coords() {
        let mut w = world();
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        let cells = [
            Pos::new(0, 0, 0),
            Pos::new(15, 15, 15),
            Pos::new(16, 16, 16),
            Pos::new(-1, -1, -1),
            Pos::new(-17, -64, 40),
            Pos::new(100, 319, -200),
        ];
        for (i, p) in cells.iter().enumerate() {
            let s = if i % 2 == 0 { stone } else { lamp };
            assert_eq!(w.set(*p, s), Some(StateId::AIR), "{p:?}");
        }
        // 格子を作り直した後も、前に書いた物が同じ座標で読める。
        for (i, p) in cells.iter().enumerate() {
            let s = if i % 2 == 0 { stone } else { lamp };
            assert_eq!(w.get(*p), s, "{p:?}");
        }
        // 隣のマスは空気のまま。
        assert_eq!(w.get(Pos::new(1, 0, 0)), StateId::AIR);
        assert_eq!(w.get(Pos::new(-2, -1, -1)), StateId::AIR);
    }

    #[test]
    fn height_bounds() {
        let mut w = world();
        let stone = block::STONE.default_state();
        assert!(w.in_bounds(Pos::new(0, -64, 0)));
        assert!(w.in_bounds(Pos::new(0, 319, 0)));
        assert!(!w.in_bounds(Pos::new(0, -65, 0)));
        assert!(!w.in_bounds(Pos::new(0, 320, 0)));
        assert_eq!(w.set(Pos::new(0, 320, 0), stone), None);
        assert_eq!(w.get(Pos::new(0, 320, 0)), StateId::AIR);
    }

    #[test]
    fn refuses_to_grow_past_the_grid_limit() {
        let mut w = world();
        let stone = block::STONE.default_state();
        assert_eq!(w.set(Pos::new(0, 0, 0), stone), Some(StateId::AIR));
        // 256 セクション (4096 マス) を超えて離れた所へは書けない。
        assert_eq!(w.set(Pos::new(16 * 256, 0, 0), stone), None);
        assert_eq!(w.set(Pos::new(16 * 255, 0, 0), stone), Some(StateId::AIR));
        assert_eq!(w.get(Pos::new(0, 0, 0)), stone);
    }

    // 書けるかの確かめは、実際に書いた結果と同じになる。確かめは何も書かず、格子も広げない。
    #[test]
    fn can_set_all_agrees_with_writing() {
        let stone = block::STONE.default_state();
        let at = |x: i32, y: i32| (Pos::new(x, y, 0), stone);
        let far = 16 * 256;
        type Cells<'a> = &'a [(Pos, StateId)];
        let cases: [(Cells, Cells, bool); 8] = [
            // (前に書いた物, 書きたい物, 全部書けるか)
            (&[], &[at(0, 0), at(far - 16, 0)], true),
            (&[], &[at(0, 0), at(far, 0)], false),
            (
                &[],
                &[at(0, 0), (Pos::new(far * 4, 0, 0), StateId::AIR)],
                true,
            ),
            (&[], &[at(0, 0), at(0, 320)], false),
            (&[at(0, 0)], &[at(far - 16, 0)], true),
            (&[at(0, 0)], &[at(far, 0)], false),
            (&[at(0, 0)], &[at(-16, 0), at(far - 32, 0)], true),
            (&[at(0, 0)], &[at(-16, 0), at(far - 16, 0)], false),
        ];
        for (i, (before, batch, expect)) in cases.iter().enumerate() {
            let mut w = world();
            for (pos, state) in before.iter() {
                assert!(w.set(*pos, *state).is_some());
            }
            let grid = (w.origin, w.size, w.sections.len(), w.pending_changes());
            assert_eq!(w.can_set_all(batch.iter().copied()), *expect, "case {i}");
            assert_eq!(
                (w.origin, w.size, w.sections.len(), w.pending_changes()),
                grid
            );
            let wrote = batch
                .iter()
                .all(|(pos, state)| w.set(*pos, *state).is_some());
            assert_eq!(wrote, *expect, "case {i}");
        }
    }

    #[test]
    fn air_into_empty_space_allocates_nothing() {
        let mut w = world();
        assert_eq!(w.set(Pos::new(5, 5, 5), StateId::AIR), Some(StateId::AIR));
        assert_eq!(w.pending_changes(), 0);
        assert!(w.sections.is_empty());
    }

    // 何回書いても、書き出すのは正味の変化だけ (19 §3.8)。
    #[test]
    fn changes_are_net() {
        let mut w = world();
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        let (a, b, c) = (Pos::new(0, 0, 0), Pos::new(1, 0, 0), Pos::new(2, 0, 0));
        w.set(a, stone);
        w.set(b, stone);
        w.set(b, lamp); // 2 回書いても 1 件
        w.set(c, stone);
        w.set(c, StateId::AIR); // 元に戻したので出ない
        assert_eq!(w.pending_changes(), 3);
        assert_eq!(
            w.drain_changes(),
            [
                Change {
                    pos: a,
                    state: stone
                },
                Change {
                    pos: b,
                    state: lamp
                }
            ]
        );
        // 書き出した後は空。印も消えているので、次の書き込みがまた記録される。
        assert_eq!(w.drain_changes(), []);
        w.set(a, lamp);
        assert_eq!(
            w.drain_changes(),
            [Change {
                pos: a,
                state: lamp
            }]
        );
    }

    #[test]
    fn same_state_write_records_nothing() {
        let mut w = world();
        let stone = block::STONE.default_state();
        w.set(Pos::new(0, 0, 0), stone);
        w.drain_changes();
        assert_eq!(w.set(Pos::new(0, 0, 0), stone), Some(stone));
        assert_eq!(w.pending_changes(), 0);
    }
}
