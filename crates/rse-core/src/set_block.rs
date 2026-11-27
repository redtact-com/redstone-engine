//! 書き込みの手順とフラグ (V04 §3.1・§4.1・§4.2・§4.10)。
//!
//! 1 回の書き込みの順は「撤去フック → 設置フック → NC → CU → 古い状態の間接 SU →
//! 6 方向の SU → 新しい状態の間接 SU」(V04 §7.2 の 1)。フックを呼ぶ条件はここに置き、素子に持たせない。
//!
//! クライアント向けの処理 (フラグ 2・4・8、再描画、POI、高さマップ、光) は持たない。
//! フラグの値は受け取って捨てる (V04 §7.3)。

use crate::level::Level;
use crate::rules::Rules;
use crate::state::{BlockId, StateId, flag};
use crate::status::Status;
use core::ops::BitOr;
use rse_java::Pos;

/// 書き込みのフラグ (V04 §3.1 の表)。ビットの値は実機と同じ。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpdateFlags(pub u16);

impl UpdateFlags {
    /// 1: NC を 6 方向へ出す。撤去フックを呼ぶ条件の片方。
    pub const NEIGHBORS: UpdateFlags = UpdateFlags(1);
    /// 2: クライアントへ送る。回路の挙動には効かない。
    pub const CLIENTS: UpdateFlags = UpdateFlags(2);
    /// 4: クライアントの再描画を止める。サーバーでは何も変えない。
    pub const INVISIBLE: UpdateFlags = UpdateFlags(4);
    /// 8: クライアントの描画の優先。サーバーでは読まれない。
    pub const IMMEDIATE: UpdateFlags = UpdateFlags(8);
    /// 16: SU を一切出さない。
    pub const KNOWN_SHAPE: UpdateFlags = UpdateFlags(16);
    /// 32: SU の結果で壊れる時、ドロップを出さない。
    pub const SUPPRESS_DROPS: UpdateFlags = UpdateFlags(32);
    /// 64: フックへ「ピストンで動いた」を真で渡す。撤去フックを呼ぶ条件の片方。
    pub const MOVE_BY_PISTON: UpdateFlags = UpdateFlags(64);
    /// 128: SU の受け手がダストなら、その SU を捨てる。
    pub const SKIP_SHAPE_UPDATE_ON_WIRE: UpdateFlags = UpdateFlags(128);
    /// 256: 古い BlockEntity を外す前の副作用を呼ばない。
    pub const SKIP_BLOCK_ENTITY_SIDEEFFECTS: UpdateFlags = UpdateFlags(256);
    /// 512: 設置フックを呼ばない。
    pub const SKIP_ON_PLACE: UpdateFlags = UpdateFlags(512);

    /// 3 = 1 + 2。ふつうの書き込み。
    pub const ALL: UpdateFlags = UpdateFlags(3);
    /// 11 = 1 + 2 + 8。
    pub const ALL_IMMEDIATE: UpdateFlags = UpdateFlags(11);
    /// 260 = 4 + 256。名前に反して SU を出す (16 が入っていない)。
    pub const NONE: UpdateFlags = UpdateFlags(260);
    /// 816 = 16 + 32 + 256 + 512。
    pub const SKIP_ALL_SIDEEFFECTS: UpdateFlags = UpdateFlags(816);

    #[inline]
    pub const fn has(self, flag: UpdateFlags) -> bool {
        self.0 & flag.0 != 0
    }

    /// `flag` のビットを落とす。
    #[inline]
    pub const fn without(self, flag: UpdateFlags) -> UpdateFlags {
        UpdateFlags(self.0 & !flag.0)
    }
}

impl BitOr for UpdateFlags {
    type Output = UpdateFlags;
    #[inline]
    fn bitor(self, rhs: UpdateFlags) -> UpdateFlags {
        UpdateFlags(self.0 | rhs.0)
    }
}

/// 形状更新の残り回数の初期値 (V04 §3.2。Block の UPDATE_LIMIT)。
pub const UPDATE_LIMIT: i32 = 512;

impl<R: Rules> Level<R> {
    /// 書き込む。残り回数は 512 から始める (V04 §4.7)。
    pub fn set_block(&mut self, pos: Pos, state: StateId, flags: UpdateFlags) -> bool {
        self.set_block_with(pos, state, flags, UPDATE_LIMIT)
    }

    /// 書き込む (V04 §4.1。Level.setBlock)。`left` は形状更新の残り回数。
    ///
    /// 偽を返すのは、書けなかった時と、変化が無かった時。その時は通知を 1 つも出さない。
    pub fn set_block_with(
        &mut self,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    ) -> bool {
        // 手順 1: 高さの外。
        if !self.world.in_bounds(pos) {
            return false;
        }
        // 手順 3・4: チャンクへの書き込み。
        let Some(old) = self.write_and_run_hooks(pos, state, flags) else {
            return false;
        };
        // 手順 5: 読み直す。フックの中で同じ座標が書き換えられていたら、何も通知しない。
        // 最外でフックが連鎖を回し切る場合に効く (V04 §5.5)。
        if self.world.get(pos) != state {
            return true;
        }
        // 手順 8: NC。送り元は古いブロック (V04 §5.4)。続けて CU。送り元は新しいブロック。
        if flags.has(UpdateFlags::NEIGHBORS) {
            self.update_neighbors_at(pos, old.block(), None);
            if state.has(flag::ANALOG_OUTPUT) {
                self.update_neighbour_for_output_signal(pos, state.block());
            }
        }
        // 手順 9: SU。フラグから 1 と 32 を落とし、残り回数を 1 減らして渡す。
        // 1 を落とすので、SU の結果で起きた書き込みは NC を出さない (V04 §5.3)。
        if !flags.has(UpdateFlags::KNOWN_SHAPE) && left > 0 {
            let passed = flags
                .without(UpdateFlags::NEIGHBORS)
                .without(UpdateFlags::SUPPRESS_DROPS);
            R::update_indirect_shapes(self, pos, old, passed, left - 1);
            self.update_neighbour_shapes(pos, state, passed, left - 1);
            R::update_indirect_shapes(self, pos, state, passed, left - 1);
        }
        true
    }

    /// チャンクへの書き込みとフック (V04 §4.2。LevelChunk.setBlockState)。
    /// 変化があれば古い状態を返す。`None` は「無し」(通知を出さない)。
    fn write_and_run_hooks(
        &mut self,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
    ) -> Option<StateId> {
        // 手順 1〜3: 同じ状態なら何もしない。状態は番号なので、整数の比較で済む (V04 §5.1)。
        let old = self.world.get(pos);
        if old == state {
            return None;
        }
        if self.world.set(pos, state).is_none() {
            // 格子の枠 (エンジンの決めた広さ) の外で、書けなかった。実機なら書ける。
            self.raise(Status::SAFETY);
            return None;
        }
        let block_changed = old.block() != state.block();
        let moved = flags.has(UpdateFlags::MOVE_BY_PISTON);
        // 手順 6・10: BlockEntity の撤去と作成。表はまだ無い (席)。
        if old.has(flag::HAS_BE) || state.has(flag::HAS_BE) {
            self.raise(Status::UNIMPLEMENTED);
        }
        // 手順 7: 撤去フック。「ブロックが変わった、または新しいブロックがレール系」かつ
        // 「フラグ 1 か 64 がある」時だけ呼ぶ。フラグ 2 だけの置き換えでは呼ばない (V04 §5.8)。
        if (block_changed || state.behavior().is_rail())
            && (flags.has(UpdateFlags::NEIGHBORS) || moved)
        {
            R::on_remove(self, pos, old, moved);
        }
        // 手順 8: 読み直す。撤去フックの中で別のブロックに変わっていたら「無し」。
        if self.world.get(pos).block() != state.block() {
            return None;
        }
        // 手順 9: 設置フック。プロパティだけ変わった時も呼ぶ。フラグ 512 があれば呼ばない。
        if !flags.has(UpdateFlags::SKIP_ON_PLACE) {
            R::on_place(self, pos, state, old, moved);
        }
        Some(old)
    }

    /// 取り除く (V04 §4.10。Level.removeBlock)。流体が無いので、空気をフラグ 3 で書く。
    /// 「ピストンで動いた」が真なら 64 を足す。
    pub fn remove_block(&mut self, pos: Pos, moved: bool) -> bool {
        let flags = if moved {
            UpdateFlags::ALL | UpdateFlags::MOVE_BY_PISTON
        } else {
            UpdateFlags::ALL
        };
        self.set_block(pos, StateId::AIR, flags)
    }

    /// 壊す (V04 §4.10。Level.destroyBlock)。今の状態が空気なら偽。
    /// 空気をフラグ 3 と、渡された残り回数で書く。だから撤去フックも NC も出る。
    /// ドロップ・パーティクル・ゲームイベントは持たない。
    pub fn destroy_block(&mut self, pos: Pos, drop: bool, left: i32) -> bool {
        let _ = drop;
        if self.world.get(pos).is_air() {
            return false;
        }
        self.set_block_with(pos, StateId::AIR, UpdateFlags::ALL, left)
    }

    /// 6 方向の NC を出す (V04 §4.5。ServerLevel.updateNeighborsAt)。6 方向で 1 件になる。
    pub fn update_neighbors_at(
        &mut self,
        center: Pos,
        src: BlockId,
        ori: Option<crate::update::Orientation>,
    ) {
        self.add_update(crate::update::Update::multi(center, src, None, ori));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::block;
    use crate::testing::{Act, Ev, level, lit_lamp};
    use rse_java::Dir::{self, *};

    const P: Pos = Pos::new(0, 0, 0);

    // フラグのビットは実機と同じ値 (V04 §3.1)。
    #[test]
    fn flag_bits_are_fixed() {
        let bits = [
            UpdateFlags::NEIGHBORS,
            UpdateFlags::CLIENTS,
            UpdateFlags::INVISIBLE,
            UpdateFlags::IMMEDIATE,
            UpdateFlags::KNOWN_SHAPE,
            UpdateFlags::SUPPRESS_DROPS,
            UpdateFlags::MOVE_BY_PISTON,
            UpdateFlags::SKIP_SHAPE_UPDATE_ON_WIRE,
            UpdateFlags::SKIP_BLOCK_ENTITY_SIDEEFFECTS,
            UpdateFlags::SKIP_ON_PLACE,
        ]
        .map(|f| f.0);
        assert_eq!(bits, [1, 2, 4, 8, 16, 32, 64, 128, 256, 512]);
        assert_eq!(UpdateFlags::ALL.0, 3);
        assert_eq!(UpdateFlags::ALL_IMMEDIATE.0, 11);
        assert_eq!(UpdateFlags::NONE.0, 260);
        assert_eq!(UpdateFlags::SKIP_ALL_SIDEEFFECTS.0, 816);
        assert_eq!(UPDATE_LIMIT, 512);
    }

    fn nc(pos: Pos, src: BlockId) -> Ev {
        Ev::Nc { pos, src }
    }
    fn su(pos: Pos, dir: Dir, other: StateId) -> Ev {
        Ev::Su { pos, dir, other }
    }

    // 別のブロックへ、フラグ 3: 撤去フック → 設置フック → NC → 古い状態の間接 SU →
    // 6 方向の SU → 新しい状態の間接 SU (V04 §7.2 の 1)。
    // NC は西・東・下・上・北・南、SU は西・東・北・南・下・上 (V04 §7.2 の 2・3)。
    // NC の送り元は古いブロック (V04 §5.4)。SU の方向は「受け手から見た相手の方向」(V04 §4.6)。
    #[test]
    fn flag3_replace_runs_in_fixed_order() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        lv.set_state_raw(P, stone);
        assert!(lv.set_block(P, lamp, UpdateFlags::ALL));
        let mut expect = vec![
            Ev::Remove {
                pos: P,
                old: stone,
                moved: false,
            },
            Ev::Place {
                pos: P,
                state: lamp,
                old: stone,
                moved: false,
            },
        ];
        for d in [West, East, Down, Up, North, South] {
            expect.push(nc(P.offset(d), block::STONE));
        }
        expect.push(Ev::Indirect {
            pos: P,
            state: stone,
            flags: UpdateFlags(2),
            left: 511,
        });
        for d in [West, East, North, South, Down, Up] {
            expect.push(su(P.offset(d), d.opposite(), lamp));
        }
        expect.push(Ev::Indirect {
            pos: P,
            state: lamp,
            flags: UpdateFlags(2),
            left: 511,
        });
        assert_eq!(lv.rules.log, expect);
        assert!(lv.take_status().is_ok());
    }

    // 同じ状態を書くと何も起きない (V04 §5.1)。
    #[test]
    fn same_state_does_nothing() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        lv.set_state_raw(P, stone);
        assert!(!lv.set_block(P, stone, UpdateFlags::ALL));
        assert!(!lv.set_block(Pos::new(5, 5, 5), StateId::AIR, UpdateFlags::ALL));
        assert_eq!(lv.rules.log, []);
    }

    #[test]
    fn out_of_height_is_refused() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        assert!(!lv.set_block(Pos::new(0, 320, 0), stone, UpdateFlags::ALL));
        assert!(!lv.set_block(Pos::new(0, -65, 0), stone, UpdateFlags::ALL));
        assert_eq!(lv.rules.log, []);
        assert!(lv.take_status().is_ok());
    }

    /// フックだけを抜き出す。
    fn hooks(log: &[Ev]) -> Vec<&'static str> {
        log.iter()
            .filter_map(|e| match e {
                Ev::Remove { .. } => Some("remove"),
                Ev::Place { .. } => Some("place"),
                _ => None,
            })
            .collect()
    }
    fn count_nc(log: &[Ev]) -> usize {
        log.iter().filter(|e| matches!(e, Ev::Nc { .. })).count()
    }
    fn count_su(log: &[Ev]) -> usize {
        log.iter().filter(|e| matches!(e, Ev::Su { .. })).count()
    }

    // フックが呼ばれる条件の表 (V04 §4.2)。
    #[test]
    fn hook_conditions_table() {
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        // (古い状態, 新しい状態, フラグ, フック, NC の数, SU の数)
        type Case = (StateId, StateId, u16, &'static [&'static str], usize, usize);
        let cases: [Case; 8] = [
            // 別のブロックへ、フラグ 3
            (stone, lamp, 3, &["remove", "place"], 6, 6),
            // 別のブロックへ、フラグ 2: 撤去フックを呼ばない。NC は出ず、SU は出る
            (stone, lamp, 2, &["place"], 0, 6),
            // 別のブロックへ、フラグ 67 (1+2+64)
            (stone, lamp, 67, &["remove", "place"], 6, 6),
            // 別のブロックへ、フラグ 324 (4+64+256): 64 があるので撤去フックを呼ぶ。NC は出ない
            (stone, lamp, 324, &["remove", "place"], 0, 6),
            // 別のブロックへ、フラグ 276 (4+16+256): SU も出ない
            (stone, lamp, 276, &["place"], 0, 0),
            // 同じブロックでプロパティだけ、フラグ 3: 撤去フックを呼ばない
            (lamp, lit_lamp(), 3, &["place"], 6, 6),
            // 同じブロックでプロパティだけ、フラグ 2
            (lamp, lit_lamp(), 2, &["place"], 0, 6),
            // フラグ 512 付き: 設置フックを呼ばない
            (stone, lamp, 3 | 512, &["remove"], 6, 6),
        ];
        for (old, new, flags, expect_hooks, expect_nc, expect_su) in cases {
            let mut lv = level();
            lv.set_state_raw(P, old);
            assert!(lv.set_block(P, new, UpdateFlags(flags)));
            let log = &lv.rules.log;
            assert_eq!(hooks(log), expect_hooks, "フラグ {flags}");
            assert_eq!(count_nc(log), expect_nc, "フラグ {flags}");
            assert_eq!(count_su(log), expect_su, "フラグ {flags}");
        }
    }

    // フラグ 64 は、フックへ「ピストンで動いた」を真で渡す (V04 §3.1)。
    #[test]
    fn moved_flag_reaches_hooks() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        lv.set_state_raw(P, stone);
        lv.set_block(P, lamp, UpdateFlags(67));
        assert_eq!(
            lv.rules.log[..2],
            [
                Ev::Remove {
                    pos: P,
                    old: stone,
                    moved: true
                },
                Ev::Place {
                    pos: P,
                    state: lamp,
                    old: stone,
                    moved: true
                },
            ]
        );
    }

    // 残り回数が 0 なら SU を出さない。書き込みとフックは起きる (V04 §4.7 の表の 513 世代目)。
    #[test]
    fn no_shape_updates_when_budget_is_zero() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        assert!(lv.set_block_with(P, lamp, UpdateFlags::ALL, 0));
        assert_eq!(lv.get_state(P), lamp);
        assert_eq!(hooks(&lv.rules.log), ["remove", "place"]);
        assert_eq!(count_nc(&lv.rules.log), 6);
        assert_eq!(count_su(&lv.rules.log), 0);
        assert!(
            !lv.rules
                .log
                .iter()
                .any(|e| matches!(e, Ev::Indirect { .. }))
        );
    }

    // 設置フックが自分の座標を書き換えたら、外側の書き込みは NC も SU も出さず、真を返す
    // (V04 §4.1 の手順 5・§5.5 のオブザーバーの例)。内側はフラグ 18 (2+16) なので SU を出さない。
    #[test]
    fn rewrite_in_place_hook_suppresses_outer_updates() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        lv.rules
            .on_place
            .insert(P, vec![Act::Set(P, lamp, UpdateFlags(18))]);
        assert!(lv.set_block(P, lit_lamp(), UpdateFlags::ALL));
        assert_eq!(lv.get_state(P), lamp);
        assert_eq!(hooks(&lv.rules.log), ["remove", "place", "place"]);
        assert_eq!(count_nc(&lv.rules.log), 0);
        assert_eq!(count_su(&lv.rules.log), 0);
    }

    // 撤去フックの中で別のブロックに変わっていたら、設置フックも通知も無い (V04 §4.2 の手順 8)。
    #[test]
    fn replaced_in_remove_hook_returns_none() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        lv.set_state_raw(P, stone);
        // 撤去フックが、フックも更新も出さないフラグ (816) でレバーを置く。
        let lever = block::LEVER.default_state();
        lv.rules
            .on_remove
            .insert(P, vec![Act::Set(P, lever, UpdateFlags(816))]);
        assert!(!lv.set_block(P, lamp, UpdateFlags::ALL));
        assert_eq!(lv.get_state(P), lever);
        assert_eq!(hooks(&lv.rules.log), ["remove"]);
        assert_eq!(count_nc(&lv.rules.log), 0);
        assert_eq!(count_su(&lv.rules.log), 0);
    }

    // 取り除く: 空気をフラグ 3 (動いたなら 67) で書く (V04 §4.10)。
    #[test]
    fn remove_block_writes_air_with_flag3() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        lv.set_state_raw(P, stone);
        assert!(lv.remove_block(P, true));
        assert_eq!(lv.get_state(P), StateId::AIR);
        assert_eq!(
            lv.rules.log[0],
            Ev::Remove {
                pos: P,
                old: stone,
                moved: true
            }
        );
        assert_eq!(count_nc(&lv.rules.log), 6);
        // 何も無い所を取り除いても、何も起きない。
        assert!(!lv.remove_block(Pos::new(3, 3, 3), false));
    }

    #[test]
    fn destroy_block_is_false_on_air() {
        let mut lv = level();
        assert!(!lv.destroy_block(P, true, 512));
        lv.set_state_raw(P, block::STONE.default_state());
        assert!(lv.destroy_block(P, true, 512));
        assert_eq!(lv.get_state(P), StateId::AIR);
        assert_eq!(hooks(&lv.rules.log), ["remove", "place"]);
    }

    // 正味の変化が書き出しに出る (19 §3.8)。
    #[test]
    fn writes_show_up_in_changes() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        lv.set_block(P, stone, UpdateFlags::ALL);
        lv.set_block(P, lit_lamp(), UpdateFlags::CLIENTS);
        assert_eq!(
            lv.drain_changes(),
            [crate::world::Change {
                pos: P,
                state: lit_lamp()
            }]
        );
    }
}
