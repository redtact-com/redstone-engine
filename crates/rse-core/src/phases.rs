//! 相の表と 1 tick (V01 §2.3・§7.1・19 §4.1)。
//!
//! 相の順は定数の配列 [`PHASES`] で持ち、単体テストで固定する。[`Level::tick`] はこの配列を
//! 先頭から回すだけで、順を別の所に書かない。
//!
//! 凍結 (V01 §4.8) は相を止めるだけである。積む側 (予約・ブロックイベント・入力の連鎖) は止めない。

use crate::level::Level;
use crate::rules::Rules;
use crate::status::Status;

/// 相。名前は V01 §2.2・§2.3 の番号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 時計を進める。
    B2,
    /// handling_tick を真にする。
    L0,
    /// 天候。
    L2,
    /// 空の明るさ。
    L4,
    /// ゲーム時刻を 1 増やす。
    L5,
    /// ブロックの予約 tick。集める → 回す。
    L6,
    /// 流体の予約 tick。L6 が全部終わってから集める。
    L7,
    /// ランダム tick。
    L9,
    /// ブロックイベントを空になるまで回す。
    L10,
    /// handling_tick を偽にする。
    L11,
    /// エンティティ (在席の検知)。
    L12,
    /// BlockEntity。pending を移す → ticker を登録順に回す → 除去を詰める。
    L13,
}

/// 1 tick の相の順 (19 §4.1 の表)。
pub const PHASES: [Phase; 12] = [
    Phase::B2,
    Phase::L0,
    Phase::L2,
    Phase::L4,
    Phase::L5,
    Phase::L6,
    Phase::L7,
    Phase::L9,
    Phase::L10,
    Phase::L11,
    Phase::L12,
    Phase::L13,
];

/// 凍結中の相の扱い。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Frozen {
    /// 飛ばす。
    Skip,
    /// ふだんどおり動く。
    Run,
    /// 掃除だけ動く。
    CleanupOnly,
}

impl Phase {
    /// 凍結中にどう扱うか (V01 §2.3 の表の「凍結中」の列・§7.1)。
    /// 時計と、L4 以外の中身のある相を飛ばす。BlockEntity の列は掃除だけする。
    /// L12 は実機ではプレイヤーだけ動く。プレイヤーの在席は入力 (区間 B5) で受けるので、相としては飛ばす。
    pub const fn when_frozen(self) -> Frozen {
        match self {
            Phase::L0 | Phase::L4 | Phase::L11 => Frozen::Run,
            Phase::L13 => Frozen::CleanupOnly,
            Phase::B2
            | Phase::L2
            | Phase::L5
            | Phase::L6
            | Phase::L7
            | Phase::L9
            | Phase::L10
            | Phase::L12 => Frozen::Skip,
        }
    }
}

impl<R: Rules> Level<R> {
    /// 1 tick 進める。戻り値は、この tick の間 (と、前に取り出した後) に立った状態コード。
    pub fn tick(&mut self) -> Status {
        for phase in PHASES {
            self.run_phase(phase, false);
        }
        self.take_status()
    }

    /// 凍結中の 1 tick (V01 §4.8)。ゲーム時刻は進まず、予約 tick もブロックイベントも走らない。
    pub fn tick_frozen(&mut self) -> Status {
        for phase in PHASES {
            match phase.when_frozen() {
                Frozen::Skip => {}
                Frozen::Run => self.run_phase(phase, false),
                Frozen::CleanupOnly => self.run_phase(phase, true),
            }
        }
        self.take_status()
    }

    /// 相を 1 つ回す。席だけの相も、match の腕としてここに並べる。
    fn run_phase(&mut self, phase: Phase, cleanup_only: bool) {
        match phase {
            // 縦切りでは値だけ (19 §4.1)。
            Phase::B2 => self.clock += 1,
            Phase::L0 => self.handling_tick = true,
            // 空の枠。天候を持つ素子 (避雷針・雨の検知) が入る時に中身を足す。
            Phase::L2 => {}
            // 外から与えた値を読む (19 §4.1)。
            Phase::L4 => self.sky_darken = self.sky_darken_input,
            Phase::L5 => self.game_time += 1,
            Phase::L6 => self.run_block_ticks(),
            // 空の枠。流体の一式を足す時、L6 が全部終わった後のここで集める (V02 §7.2 の 5)。
            Phase::L7 => {}
            // 空の枠。既定の速度は 0 (19 §4.1)。
            Phase::L9 => {}
            Phase::L10 => self.run_block_events(),
            Phase::L11 => self.handling_tick = false,
            // 空の枠。プレイヤー以外の在席の検知が入る (19 §4.1)。
            Phase::L12 => {}
            // 空の枠。BlockEntity の表と ticker の列は、BlockEntity を持つ素子の PR で足す (19 §3.3)。
            Phase::L13 => {
                let _ = cleanup_only;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Phase::*;
    use super::*;
    use crate::state::block;
    use crate::testing::{Act, Ev, level};
    use crate::ticks::TickPriority;
    use rse_java::Pos;

    // 19 §4.1 の表の順。この順は 1.21.1・26.2・26.3・26.4-pre-1 で同じ (V01 §2.3)。
    #[test]
    fn phase_table_is_fixed() {
        assert_eq!(PHASES, [B2, L0, L2, L4, L5, L6, L7, L9, L10, L11, L12, L13]);
    }

    // 凍結中に動く相 (V01 §2.3 の表の「凍結中」の列)。
    #[test]
    fn frozen_table_is_fixed() {
        let run: Vec<Phase> = PHASES
            .into_iter()
            .filter(|p| p.when_frozen() == Frozen::Run)
            .collect();
        assert_eq!(run, [L0, L4, L11]);
        let cleanup: Vec<Phase> = PHASES
            .into_iter()
            .filter(|p| p.when_frozen() == Frozen::CleanupOnly)
            .collect();
        assert_eq!(cleanup, [L13]);
    }

    // 1 tick が相の表どおりに回る。予約 tick (L6) はゲーム時刻を進めた (L5) 後で、
    // ブロックイベント (L10) はその後。handling_tick は L0 から L11 の手前まで真 (V03 §7.3)。
    #[test]
    fn one_tick_follows_the_table() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP;
        let (a, b) = (Pos::new(0, 0, 0), Pos::new(1, 0, 0));
        lv.set_state_raw(a, lamp.default_state());
        lv.set_state_raw(b, lamp.default_state());
        lv.schedule_tick(a, lamp, 1, TickPriority::Normal);
        lv.block_event(b, lamp, 0, 0);
        lv.rules.on_tick.insert(a, vec![Act::Snapshot]);
        lv.rules.on_event.insert(b, vec![Act::Snapshot]);
        assert!(!lv.handling_tick());
        assert_eq!(lv.tick(), Status::OK);
        assert!(!lv.handling_tick());
        assert_eq!((lv.game_time(), lv.clock()), (1, 1));
        assert_eq!(
            lv.rules.log,
            [
                Ev::Tick {
                    pos: a,
                    state: lamp.default_state(),
                    time: 1
                },
                Ev::Snapshot {
                    time: 1,
                    handling_tick: true
                },
                Ev::Event {
                    pos: b,
                    state: lamp.default_state(),
                    a: 0,
                    b: 0
                },
                Ev::Snapshot {
                    time: 1,
                    handling_tick: true
                },
            ]
        );
    }

    // 空の明るさは、外から与えた値を L4 で読む (19 §4.1)。凍結中も L4 は動く。
    #[test]
    fn sky_darken_is_read_at_l4() {
        let mut lv = level();
        lv.set_sky_darken(7);
        assert_eq!(lv.sky_darken(), 0);
        lv.tick_frozen();
        assert_eq!(lv.sky_darken(), 7);
        lv.set_sky_darken(3);
        lv.tick();
        assert_eq!(lv.sky_darken(), 3);
    }

    // 凍結中は時計もゲーム時刻も進まず、予約 tick とブロックイベントは溜まるだけ (V01 §4.8・V03 §4.2)。
    // 凍結を解いた最初の tick で、溜まっていたブロックイベントが積んだ順にまとめて回る。
    #[test]
    fn frozen_tick_keeps_queues() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP;
        let a = Pos::new(0, 0, 0);
        lv.set_state_raw(a, lamp.default_state());
        lv.schedule_tick(a, lamp, 1, TickPriority::Normal);
        for _ in 0..3 {
            lv.block_event(a, lamp, lv.pending_block_events() as i32, 0);
            assert_eq!(lv.tick_frozen(), Status::OK);
        }
        assert_eq!((lv.game_time(), lv.clock()), (0, 0));
        assert_eq!(lv.rules.log, []);
        assert_eq!(lv.scheduled_tick_count(), 1);
        assert_eq!(lv.pending_block_events(), 3);
        lv.tick();
        let kinds: Vec<i32> = lv
            .rules
            .log
            .iter()
            .map(|e| match e {
                Ev::Tick { .. } => -1,
                Ev::Event { a, .. } => *a,
                _ => -2,
            })
            .collect();
        assert_eq!(kinds, [-1, 0, 1, 2]);
    }
}
