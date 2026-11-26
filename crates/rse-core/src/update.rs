//! 近隣更新 (NC)・形状更新 (SU)・コンパレーター更新 (CU) の配り方 (V04 §4.3〜§4.8・19 §3.6)。
//!
//! 3 種の更新は、全部 1 本のスタックを通る。
//! - 最外で出した更新は、その場で空になるまで回し切る。相ごとにまとめない (V04 §7.5)。
//! - 更新の実行中に出した更新は、「この層で追加された分」に溜めるだけで戻る。
//!   今の 1 件の受け手が戻った直後に、積んだ順で走る。そのあとで親の残りに戻る。
//!   つまり深さ優先で、同じ層の中は先入れ先出し (V04 §2.2)。
//!
//! 実機の更新器は Level ごとに 1 個で、ここでも [`Level`] が 1 個持つ。

use crate::level::Level;
use crate::rules::Rules;
use crate::set_block::UpdateFlags;
use crate::state::{Behavior, BlockId, StateId};
use crate::status::Status;
use alloc::vec::Vec;
use rse_java::{Dir, Pos};

/// NC に付いて回る「向きの文脈」(V04 §4.9)。48 個の表の添字。
/// 既定の設定 (実験なし) では常に「無し」で、挙動に効かない。席だけ置く。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Orientation(pub u8);

/// 更新 1 件 (V04 §3.4 の 4 種類)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Update {
    /// 受け手 1 つへの NC。実行の時点で受け手の状態を読む。
    Simple {
        pos: Pos,
        src: BlockId,
        ori: Option<Orientation>,
    },
    /// 受け手 1 つへの NC。積んだ時点の状態で呼び、読み直さない (V04 §5.6)。CU が使う。
    Full {
        state: StateId,
        pos: Pos,
        src: BlockId,
        ori: Option<Orientation>,
        moved: bool,
    },
    /// 6 方向の NC。1 回の実行で 1 方向へ届け、方向が残っている間はスタックに残る。
    Multi {
        center: Pos,
        src: BlockId,
        skip: Option<Dir>,
        ori: Option<Orientation>,
        /// 次に届ける方向の、NC の並びの中の添字。
        next: u8,
    },
    /// 1 方向の SU。`other` は積んだ時点の相手の状態。受け手の状態は実行の時点で読む。
    Shape {
        /// 受け手から見た相手の方向。
        dir: Dir,
        other: StateId,
        pos: Pos,
        other_pos: Pos,
        flags: UpdateFlags,
        left: i32,
    },
}

impl Update {
    /// 6 方向の NC を作る。最初の方向 (西) が除外方向なら、添字を 1 進めておく (V04 §4.5 の 1)。
    pub(crate) fn multi(
        center: Pos,
        src: BlockId,
        skip: Option<Dir>,
        ori: Option<Orientation>,
    ) -> Update {
        let next = if skip == Some(Dir::NC_ORDER[0]) { 1 } else { 0 };
        Update::Multi {
            center,
            src,
            skip,
            ori,
            next,
        }
    }

    /// 捨てた時に記録する座標。
    fn anchor(&self) -> Pos {
        match *self {
            Update::Simple { pos, .. } | Update::Full { pos, .. } | Update::Shape { pos, .. } => {
                pos
            }
            Update::Multi { center, .. } => center,
        }
    }
}

/// 更新器の状態 (V04 §3.3)。
pub struct Updater {
    /// 実行待ちの更新。末尾が上。
    stack: Vec<Update>,
    /// 今走っている 1 件の中で追加された更新 (追加順)。
    added: Vec<Update>,
    /// 最外の呼び出しが始まってから受け取った件数 (捨てた分も数える)。0 なら実行中でない。
    count: u32,
    /// 上限。負は無制限。
    limit: i32,
    /// 上限で最初に捨てた更新の座標。1 回だけ記録する (V18 §7.4)。
    first_dropped: Option<Pos>,
}

impl Updater {
    pub(crate) fn new(limit: i32) -> Updater {
        Updater {
            stack: Vec::new(),
            added: Vec::new(),
            count: 0,
            limit,
            first_dropped: None,
        }
    }
}

impl<R: Rules> Level<R> {
    /// 更新を 1 件受ける (V04 §4.3。CollectingNeighborUpdater.addAndRun)。
    ///
    /// 数えるのはここを通った回数で、NC・SU・CU を区別しない。6 方向の NC は 1 件、SU は 1 方向 1 件。
    /// 上限に達したら新しい分だけ捨てる。積み済みは最後まで走り、書き込みは戻さない (V04 §5.10)。
    pub(crate) fn add_update(&mut self, update: Update) {
        let up = &mut self.updater;
        let running = up.count > 0;
        let too_many = up.limit >= 0 && up.count >= up.limit as u32;
        up.count = up.count.saturating_add(1);
        if too_many {
            if up.first_dropped.is_none() {
                up.first_dropped = Some(update.anchor());
            }
            self.status |= Status::CHAIN_LIMIT;
        } else if running {
            up.added.push(update);
        } else {
            up.stack.push(update);
        }
        if !running {
            self.run_updates();
        }
    }

    /// スタックが空になるまで回す (V04 §4.4。CollectingNeighborUpdater.runUpdates)。
    fn run_updates(&mut self) {
        while !self.updater.stack.is_empty() || !self.updater.added.is_empty() {
            // 追加された分を、末尾から順にスタックへ積む。最初に追加された 1 件が一番上に来る。
            while let Some(update) = self.updater.added.pop() {
                self.updater.stack.push(update);
            }
            // 一番上を実行する。続きがあり、実行中に何も追加されなければ、同じ更新をもう 1 回実行する。
            // 何か追加されたら外さずに戻る。追加された分が上に積まれ、先に走る。
            loop {
                let more = self.run_top();
                if !more {
                    self.updater.stack.pop();
                    break;
                }
                if !self.updater.added.is_empty() {
                    break;
                }
            }
        }
        self.updater.count = 0;
    }

    /// スタックの一番上を 1 回実行する。続きがあれば真。
    ///
    /// 実行中に出た更新は `added` に入るだけなので、スタックの一番上の位置は実行の間動かない。
    /// 6 方向の NC は、添字を先に進めてから受け手を呼ぶ (V04 §7.1。受け手が更新器へ再入するため)。
    fn run_top(&mut self) -> bool {
        let top = self.updater.stack.len() - 1;
        match self.updater.stack[top] {
            Update::Simple { pos, src, ori } => {
                let state = self.world.get(pos);
                R::neighbor_changed(self, pos, state, src, ori, false);
                false
            }
            Update::Full {
                state,
                pos,
                src,
                ori,
                moved,
            } => {
                R::neighbor_changed(self, pos, state, src, ori, moved);
                false
            }
            Update::Multi {
                center,
                src,
                skip,
                ori,
                next,
            } => {
                let dir = Dir::NC_ORDER[next as usize];
                // 次の方向が除外方向なら、もう 1 つ進める (V04 §4.5 の 5)。
                let mut after = next + 1;
                if (after as usize) < Dir::NC_ORDER.len()
                    && skip == Some(Dir::NC_ORDER[after as usize])
                {
                    after += 1;
                }
                self.updater.stack[top] = Update::Multi {
                    center,
                    src,
                    skip,
                    ori,
                    next: after,
                };
                // 受け手の状態は、この時点で読む。受け手は空気でも呼ぶ。
                let pos = center.offset(dir);
                let state = self.world.get(pos);
                R::neighbor_changed(self, pos, state, src, ori, false);
                (after as usize) < Dir::NC_ORDER.len()
            }
            Update::Shape {
                dir,
                other,
                pos,
                other_pos,
                flags,
                left,
            } => {
                self.run_shape_update(dir, other, pos, other_pos, flags, left);
                false
            }
        }
    }

    /// SU を 1 件実行する (V04 §4.6。NeighborUpdater.executeShapeUpdate と Block.updateOrDestroy)。
    fn run_shape_update(
        &mut self,
        dir: Dir,
        other: StateId,
        pos: Pos,
        other_pos: Pos,
        flags: UpdateFlags,
        left: i32,
    ) {
        // 受け手の今の状態を読む。
        let state = self.world.get(pos);
        // フラグ 128 があり、受け手がダストなら捨てる。
        if flags.has(UpdateFlags::SKIP_SHAPE_UPDATE_ON_WIRE) && state.behavior() == Behavior::Wire {
            return;
        }
        let next = R::update_shape(self, pos, state, dir, other_pos, other);
        if next == state {
            return;
        }
        if next.is_air() {
            // 空気になるなら壊す。フラグ 3 で書くので、NC も撤去フックも出る (V04 §4.10)。残り回数はそのまま。
            self.destroy_block(pos, !flags.has(UpdateFlags::SUPPRESS_DROPS), left);
        } else {
            // フラグから 32 を落として書く。残り回数はそのまま (V04 §4.7)。
            self.set_block_with(pos, next, flags.without(UpdateFlags::SUPPRESS_DROPS), left);
        }
    }

    /// 6 方向の NC を、1 方向を除いて出す (V04 §4.5。ServerLevel.updateNeighborsAtExceptFromFacing)。
    pub fn update_neighbors_at_except(
        &mut self,
        center: Pos,
        src: BlockId,
        skip: Dir,
        ori: Option<Orientation>,
    ) {
        self.add_update(Update::multi(center, src, Some(skip), ori));
    }

    /// 受け手 1 つへ NC を出す (V04 §4.5。ServerLevel.neighborChanged の 3 引数)。
    pub fn neighbor_changed(&mut self, pos: Pos, src: BlockId, ori: Option<Orientation>) {
        self.add_update(Update::Simple { pos, src, ori });
    }

    /// 受け手 1 つへ、状態を持たせた NC を出す (V04 §4.5。ServerLevel.neighborChanged の 5 引数)。
    pub fn neighbor_changed_with_state(
        &mut self,
        state: StateId,
        pos: Pos,
        src: BlockId,
        ori: Option<Orientation>,
        moved: bool,
    ) {
        self.add_update(Update::Full {
            state,
            pos,
            src,
            ori,
            moved,
        });
    }

    /// 受け手 1 つへ SU を出す (V04 §4.6。Level.neighborShapeChanged)。
    /// `dir` は受け手から見た相手の方向。ダストの間接 SU とピストンが、手で呼ぶ。
    pub fn neighbor_shape_changed(
        &mut self,
        dir: Dir,
        other: StateId,
        pos: Pos,
        other_pos: Pos,
        flags: UpdateFlags,
        left: i32,
    ) {
        self.add_update(Update::Shape {
            dir,
            other,
            pos,
            other_pos,
            flags,
            left,
        });
    }

    /// 6 方向の SU を出す (V04 §4.6。BlockStateBase.updateNeighbourShapes)。
    /// 西・東・北・南・下・上の順で、1 方向が 1 件。上限のカウンタも 6 件ぶん進む (V04 §5.9)。
    pub fn update_neighbour_shapes(
        &mut self,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    ) {
        for dir in Dir::SU_ORDER {
            self.neighbor_shape_changed(dir.opposite(), state, pos.offset(dir), pos, flags, left);
        }
    }

    /// CU を出す (V04 §4.8。Level.updateNeighbourForOutputSignal)。
    /// 北・東・南・西の順に、隣がコンパレーターならそこへ、隣が導体ならもう 1 マス先のコンパレーターへ届ける。
    /// 積んだ時点の状態を持たせる。コンパレーターの向きは見ない。
    pub fn update_neighbour_for_output_signal(&mut self, pos: Pos, src: BlockId) {
        for dir in Dir::HORIZONTAL {
            let near = pos.offset(dir);
            let state = self.world.get(near);
            if state.behavior() == Behavior::Comparator {
                self.neighbor_changed_with_state(state, near, src, None, false);
            } else if state.is_conductor() {
                let far = near.offset(dir);
                let state = self.world.get(far);
                if state.behavior() == Behavior::Comparator {
                    self.neighbor_changed_with_state(state, far, src, None, false);
                }
            }
        }
    }

    /// 上限で最初に捨てた更新の座標 (V18 §7.4)。
    pub fn first_dropped_update(&self) -> Option<Pos> {
        self.updater.first_dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Config;
    use crate::state::block;
    use crate::testing::{Act, Ev, level, level_with, lit_lamp};
    use rse_java::Dir::*;

    const P: Pos = Pos::new(0, 0, 0);

    fn nc_receivers(log: &[Ev]) -> Vec<Pos> {
        log.iter()
            .filter_map(|e| match e {
                Ev::Nc { pos, .. } => Some(*pos),
                _ => None,
            })
            .collect()
    }

    // NC の 6 方向は西・東・下・上・北・南 (V04 §7.2 の 2)。受け手は空気でも呼ぶ。
    #[test]
    fn multi_visits_in_nc_order() {
        let mut lv = level();
        lv.update_neighbors_at(P, block::STONE, None);
        let expect: Vec<Pos> = [West, East, Down, Up, North, South]
            .map(|d| P.offset(d))
            .to_vec();
        assert_eq!(nc_receivers(&lv.rules.log), expect);
        assert!(
            lv.rules
                .log
                .iter()
                .all(|e| matches!(e, Ev::Nc { src, .. } if *src == block::STONE))
        );
    }

    // 除外方向だけを飛ばす (V04 §4.5 の表)。
    #[test]
    fn multi_skips_only_the_excluded_direction() {
        for skip in Dir::ALL {
            let mut lv = level();
            lv.update_neighbors_at_except(P, block::STONE, skip, None);
            let expect: Vec<Pos> = Dir::NC_ORDER
                .into_iter()
                .filter(|d| *d != skip)
                .map(|d| P.offset(d))
                .collect();
            assert_eq!(nc_receivers(&lv.rules.log), expect, "除外 {skip:?}");
        }
    }

    // V04 §4.4 の例: 最外で P の 6 方向の NC を出す。西の受け手 W が、自分の NC の中でフラグ 3 の書き込みをする。
    // W の書き込みが出した更新 (W の 6 方向の NC と SU 6 件) は、W の NC が戻った直後に、積んだ順で走る。
    // そのあとで P の残り (東から) に戻る。
    #[test]
    fn nested_updates_run_before_the_parents_rest() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        let w = P.offset(West);
        lv.set_state_raw(w, lamp);
        lv.rules
            .on_nc
            .insert(w, vec![Act::Set(w, lit_lamp(), UpdateFlags::ALL)]);
        lv.update_neighbors_at(P, block::STONE, None);

        let mut expect = vec![
            // P の西 = W。W はこの中で自分を書き換える。フックはその場で走る。
            Ev::Nc {
                pos: w,
                src: block::STONE,
            },
            Ev::Place {
                pos: w,
                state: lit_lamp(),
                old: lamp,
                moved: false,
            },
            // 間接 SU の口も、書き込みの中でその場に呼ばれる (古い状態 → 新しい状態)。
            Ev::Indirect {
                pos: w,
                state: lamp,
                flags: UpdateFlags(2),
                left: 511,
            },
            Ev::Indirect {
                pos: w,
                state: lit_lamp(),
                flags: UpdateFlags(2),
                left: 511,
            },
        ];
        // W の 6 方向の NC。送り元は W の古いブロック (ランプ)。
        for d in [West, East, Down, Up, North, South] {
            expect.push(Ev::Nc {
                pos: w.offset(d),
                src: block::REDSTONE_LAMP,
            });
        }
        // W の SU 6 件。
        for d in [West, East, North, South, Down, Up] {
            expect.push(Ev::Su {
                pos: w.offset(d),
                dir: d.opposite(),
                other: lit_lamp(),
            });
        }
        // P の残り。
        for d in [East, Down, Up, North, South] {
            expect.push(Ev::Nc {
                pos: P.offset(d),
                src: block::STONE,
            });
        }
        assert_eq!(lv.rules.log, expect);
    }

    // 同じ層で積んだ更新は、積んだ順に走る (V04 §7.2 の 5)。
    // 1 つの NC の中で 2 回書き込むと、受け手が動くのは 2 回目の書き込みの後で、最後の状態だけを見る (V04 §5.5)。
    #[test]
    fn same_layer_is_first_in_first_out() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        let (a, b) = (Pos::new(10, 0, 0), Pos::new(20, 0, 0));
        lv.rules.on_nc.insert(
            P,
            vec![
                Act::Set(a, stone, UpdateFlags::ALL),
                Act::Set(b, stone, UpdateFlags::ALL),
                Act::Mark("書き込みの後"),
            ],
        );
        lv.neighbor_changed(P, block::STONE, None);
        // 印は、a と b の更新がどれも走る前に付く。
        let mark = lv
            .rules
            .log
            .iter()
            .position(|e| *e == Ev::Mark("書き込みの後"))
            .unwrap();
        let first_nested = lv
            .rules
            .log
            .iter()
            .position(|e| matches!(e, Ev::Nc { pos, .. } if *pos != P))
            .unwrap();
        assert!(mark < first_nested);
        // a の NC 6 件 → a の SU 6 件 → b の NC 6 件 → b の SU 6 件。
        let centers: Vec<Pos> = lv.rules.log[first_nested..]
            .iter()
            .filter_map(|e| match e {
                Ev::Nc { pos, .. } | Ev::Su { pos, .. } => Some(*pos),
                _ => None,
            })
            .collect();
        let near = |c: Pos, p: Pos| (p.x - c.x).abs() + (p.y - c.y).abs() + (p.z - c.z).abs() == 1;
        assert_eq!(centers.len(), 24);
        assert!(centers[..12].iter().all(|p| near(a, *p)));
        assert!(centers[12..].iter().all(|p| near(b, *p)));
    }

    // SU の受け手が別の状態を返すと、フラグから 1 を落として書く。NC は出ず、SU は続く (V04 §5.3)。
    // 残り回数は 1 世代で 1 減る (V04 §4.7)。
    #[test]
    fn shape_result_is_written_without_neighbor_updates() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        let stone = block::STONE.default_state();
        let e = P.offset(East);
        lv.set_state_raw(e, lamp);
        lv.rules.shape_result.insert(e, lit_lamp());
        lv.set_block(P, stone, UpdateFlags::ALL);
        assert_eq!(lv.get_state(e), lit_lamp());
        let log = &lv.rules.log;
        // E の書き込みは設置フックだけを呼ぶ (同じブロックなので撤去フックは無い)。
        let place = log
            .iter()
            .position(|ev| matches!(ev, Ev::Place { pos, .. } if *pos == e))
            .unwrap();
        // E を中心にした NC は 1 件も無い (E の隣で、P の隣でないマスへの NC が無い)。
        let far = e.offset(East);
        assert!(
            !log.iter()
                .any(|ev| matches!(ev, Ev::Nc { pos, .. } if *pos == far))
        );
        // E の SU は出る。世代が 1 つ進んだので、残り回数は 510。
        assert!(log[place..].contains(&Ev::Indirect {
            pos: e,
            state: lit_lamp(),
            flags: UpdateFlags(2),
            left: 510,
        }));
        assert!(log[place..].contains(&Ev::Su {
            pos: far,
            dir: West,
            other: lit_lamp(),
        }));
    }

    // SU の受け手が空気を返すと壊す。フラグ 3 で書くので、撤去フックも NC も出る (V04 §4.10)。
    #[test]
    fn shape_result_air_destroys_with_flag3() {
        let mut lv = level();
        let lever = block::LEVER.default_state();
        let u = P.offset(Up);
        lv.set_state_raw(u, lever);
        lv.rules.shape_result.insert(u, StateId::AIR);
        lv.set_block(P, block::STONE.default_state(), UpdateFlags::CLIENTS);
        assert_eq!(lv.get_state(u), StateId::AIR);
        let log = &lv.rules.log;
        assert!(log.contains(&Ev::Remove {
            pos: u,
            old: lever,
            moved: false
        }));
        assert!(log.contains(&Ev::Nc {
            pos: u.offset(Up),
            src: block::LEVER,
        }));
    }

    // フラグ 128 があり、受け手がダストなら、その SU を捨てる (V04 §4.6 の 2)。
    #[test]
    fn flag128_skips_shape_update_on_wire() {
        let wire = block::REDSTONE_WIRE.default_state();
        let stone = block::STONE.default_state();
        let e = P.offset(East);
        let su_at_e = |log: &[Ev]| {
            log.iter()
                .any(|ev| matches!(ev, Ev::Su { pos, .. } if *pos == e))
        };
        let mut lv = level();
        lv.set_state_raw(e, wire);
        lv.set_block(P, stone, UpdateFlags(2 | 128));
        assert!(!su_at_e(&lv.rules.log));
        // 128 が無ければ届く。受け手がダストでなければ、128 があっても届く。
        let mut lv = level();
        lv.set_state_raw(e, wire);
        lv.set_block(P, stone, UpdateFlags(2));
        assert!(su_at_e(&lv.rules.log));
        let mut lv = level();
        lv.set_state_raw(e, stone);
        lv.set_block(P, stone, UpdateFlags(2 | 128));
        assert!(su_at_e(&lv.rules.log));
    }

    // 上限の数え方 (V04 §4.3 の表)。フラグ 3 の書き込み 1 回は、NC 1 件 + SU 6 件 = 7 件 (V04 §5.9)。
    #[test]
    fn chain_limit_drops_only_new_updates() {
        let stone = block::STONE.default_state();
        let run = |limit: i32| {
            let mut lv = level_with(Config {
                max_chained_updates: limit,
                ..Config::default()
            });
            // 最外の 1 件 (P への NC) の中で、フラグ 3 の書き込みをする。合わせて 8 件。
            lv.rules.on_nc.insert(
                P,
                vec![Act::Set(Pos::new(10, 0, 0), stone, UpdateFlags::ALL)],
            );
            lv.neighbor_changed(P, block::STONE, None);
            let nc = lv
                .rules
                .log
                .iter()
                .filter(|e| matches!(e, Ev::Nc { .. }))
                .count();
            let su = lv
                .rules
                .log
                .iter()
                .filter(|e| matches!(e, Ev::Su { .. }))
                .count();
            (nc, su, lv.take_status(), lv.first_dropped_update())
        };
        // 8 件ちょうどなら全部走る。
        assert_eq!(run(8), (1 + 6, 6, Status::OK, None));
        // 無制限。
        assert_eq!(run(-1), (1 + 6, 6, Status::OK, None));
        // 7 件: 最後の SU (上の受け手) だけ捨てる。書き込みは残る。
        let (nc, su, status, dropped) = run(7);
        assert_eq!((nc, su), (7, 5));
        assert_eq!(status, Status::CHAIN_LIMIT);
        assert_eq!(dropped, Some(Pos::new(10, 1, 0)));
        // 2 件: 最外の 1 件と、書き込みの NC (6 方向で 1 件) だけ。SU は全部捨てる。
        let (nc, su, status, dropped) = run(2);
        assert_eq!((nc, su), (7, 0));
        assert_eq!(status, Status::CHAIN_LIMIT);
        assert_eq!(dropped, Some(Pos::new(9, 0, 0)));
        // 0 件: 最外の 1 件目から捨てる。更新は 1 つも走らない。
        let (nc, su, status, dropped) = run(0);
        assert_eq!((nc, su), (0, 0));
        assert_eq!(status, Status::CHAIN_LIMIT);
        assert_eq!(dropped, Some(P));
    }

    // 最外が終わるとカウンタは 0 に戻る。次の最外は、また上限まで受け付ける (V18 §4.9)。
    #[test]
    fn counter_resets_after_outermost_run() {
        let mut lv = level_with(Config {
            max_chained_updates: 1,
            ..Config::default()
        });
        for _ in 0..3 {
            lv.update_neighbors_at(P, block::STONE, None);
        }
        assert_eq!(nc_receivers(&lv.rules.log).len(), 18);
        assert!(lv.take_status().is_ok());
    }

    // 状態を持たせた NC は、積んだ時点の状態で呼び、読み直さない (V04 §5.6)。
    #[test]
    fn full_update_keeps_the_queued_state() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        let target = Pos::new(5, 0, 0);
        lv.set_state_raw(target, lamp);
        // P への NC の中で、target へ「ランプの状態を持たせた NC」を積み、そのあと target を石に変える。
        lv.rules.on_nc.insert(
            P,
            vec![
                Act::NcWithState(lamp, target),
                Act::Set(target, block::STONE.default_state(), UpdateFlags(816)),
            ],
        );
        lv.neighbor_changed(P, block::STONE, None);
        assert!(lv.rules.log.contains(&Ev::NcState {
            pos: target,
            state: lamp
        }));
    }

    // CU は北・東・南・西の順。直接のコンパレーターか、導体 1 個越しのコンパレーターにだけ届く (V04 §4.8)。
    // 縦切りの表にコンパレーターは無いので、届け先が無いことだけを確かめる。
    #[test]
    fn output_signal_update_reaches_nothing_without_comparators() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        for d in Dir::HORIZONTAL {
            lv.set_state_raw(P.offset(d), stone);
        }
        lv.update_neighbour_for_output_signal(P, block::STONE);
        assert_eq!(lv.rules.log, []);
    }
}
