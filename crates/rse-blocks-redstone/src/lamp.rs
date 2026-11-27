//! レッドストーンランプ (V11 §4.1・§5.1)。
//!
//! - 信号源ではない。導体なので、強充電されると周りへ見せる (これは骨格の信号の問い合わせが受け持つ)。
//! - 点灯はその場で、消灯は 4gt の予約 tick。
//! - 書き込みはフラグ 2。NC は出さず、SU だけ出す (V11 §5.3)。

use rse_core::state::block;
use rse_core::{Level, Pos, Rules, StateId, TickPriority, UpdateFlags};

// プロパティの添字 (lit だけ) と、真偽の値の添字 (true・false)。
const LIT: usize = 0;
const TRUE: u8 = 0;
const FALSE: u8 = 1;

/// 消灯までの遅延 (V11 §4.1)。
pub const OFF_DELAY: i32 = 4;

/// 点いているか。
#[inline]
pub fn is_lit(state: StateId) -> bool {
    state.value(LIT) == TRUE
}

/// 設置の時の状態 (V11 §4.1。RedstoneLampBlock.getStateForPlacement)。lit は、置く位置が受電しているかで決まる。
pub fn placement_state<R: Rules>(lv: &Level<R>, pos: Pos) -> StateId {
    let lit = if lv.has_neighbor_signal(pos) {
        TRUE
    } else {
        FALSE
    };
    block::REDSTONE_LAMP.default_state().with_value(LIT, lit)
}

/// NC を受ける (V11 §4.1。RedstoneLampBlock.neighborChanged)。
///
/// 1. lit と受電を比べる。同じなら終わり。信号の強さは見ない。
/// 2. 消えているのに受電している → その場で lit=true をフラグ 2 で書く。
/// 3. 点いているのに受電していない → 4gt の予約 tick (優先度 0) を積むだけ。その場では消さない。
///
/// 同じ位置・同じ種類の予約は 1 件しか入らないので、4gt は「予約が無い状態で、点灯中かつ無受電の NC を
/// 受けた最初の時刻」から数える。窓の中で入力が戻ってまた切れても延びない (V11 §5.1)。
/// [実機 fixture lamp-off-delay: t10 にレバーを戻す → t14 に lit=false]
pub fn neighbor_changed<R: Rules>(lv: &mut Level<R>, pos: Pos, state: StateId) {
    let lit = is_lit(state);
    let powered = lv.has_neighbor_signal(pos);
    if lit == powered {
        return;
    }
    if lit {
        lv.schedule_tick(pos, block::REDSTONE_LAMP, OFF_DELAY, TickPriority::Normal);
    } else {
        lv.set_block(pos, state.with_value(LIT, TRUE), UpdateFlags::CLIENTS);
    }
}

/// 予約 tick (V11 §4.1。RedstoneLampBlock.tick)。
/// 点いていて、その時点で受電していなければ、lit=false をフラグ 2 で書く。受電していれば何もしない (積み直さない)。
pub fn scheduled_tick<R: Rules>(lv: &mut Level<R>, pos: Pos, state: StateId) {
    if is_lit(state) && !lv.has_neighbor_signal(pos) {
        lv.set_block(pos, state.with_value(LIT, FALSE), UpdateFlags::CLIENTS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, level, put, stone};
    use rse_core::{Dir, Input, InputSection};

    // 石の床の上に、レバー (x=0) とランプ (x=1) を並べる。
    const LEVER: Pos = Pos::new(0, 1, 0);
    const LAMP: Pos = Pos::new(1, 1, 0);

    fn bench() -> Level<crate::testing::Bench> {
        let mut lv = level();
        put(&mut lv, LEVER.offset(Dir::Down), stone());
        put(&mut lv, LAMP.offset(Dir::Down), stone());
        put(
            &mut lv,
            LEVER,
            StateId::parse("lever[face=floor,facing=north,powered=false]").unwrap(),
        );
        put(&mut lv, LAMP, block::REDSTONE_LAMP.default_state());
        lv
    }

    fn toggle(lv: &mut Level<crate::testing::Bench>) {
        lv.input(InputSection::A, Input::Use { pos: LEVER });
    }

    #[test]
    fn property_slots_match_the_table() {
        assert_eq!(block::REDSTONE_LAMP.prop("lit"), Some(LIT));
        assert!(is_lit(StateId::parse("redstone_lamp[lit=true]").unwrap()));
        assert!(!is_lit(block::REDSTONE_LAMP.default_state()));
    }

    // 点灯はその場。予約は積まない (V11 §4.1 の 2)。
    #[test]
    fn turns_on_at_once() {
        let mut lv = bench();
        toggle(&mut lv);
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        assert_eq!(lv.scheduled_tick_count(), 0);
    }

    // 消灯は 4gt 後。入力で積んだ予約は「今のゲーム時刻 + 4」に走る (V11 §4.1 の 3・V01 §2.4)。
    #[test]
    fn turns_off_four_ticks_later() {
        let mut lv = bench();
        toggle(&mut lv);
        lv.tick();
        toggle(&mut lv);
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        assert_eq!(lv.scheduled_tick_count(), 1);
        for _ in 0..3 {
            lv.tick();
            assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        }
        lv.tick();
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=false]");
        assert_eq!(lv.scheduled_tick_count(), 0);
    }

    // 4gt は最後の立ち下がりから数えない (V11 §5.1)。
    // t0 で切る → t1 で戻す → t3 でまた切る。t3 の予約は捨てられ、t0 の予約が t4 に走って消える。
    #[test]
    fn off_delay_is_not_extended_by_a_second_falling_edge() {
        let mut lv = bench();
        toggle(&mut lv); // 点ける
        toggle(&mut lv); // t0: 切る
        lv.tick(); // t1
        toggle(&mut lv); // 戻す
        lv.tick(); // t2
        lv.tick(); // t3
        toggle(&mut lv); // また切る
        assert_eq!(lv.scheduled_tick_count(), 1);
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        lv.tick(); // t4
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=false]");
    }

    // 予約が走る時点で受電していれば、消えない。予約も積み直さない (V11 §4.1 の ST)。
    #[test]
    fn stays_on_if_powered_when_the_tick_fires() {
        let mut lv = bench();
        toggle(&mut lv);
        toggle(&mut lv); // 切る (予約)
        lv.tick();
        toggle(&mut lv); // 戻す
        for _ in 0..6 {
            lv.tick();
        }
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        assert_eq!(lv.scheduled_tick_count(), 0);
    }

    // ランプの書き込みはフラグ 2 なので、NC を出さない (V11 §5.3)。
    // 隣のランプは、点いたランプからは何も受け取らない (ランプは信号源でもない)。
    #[test]
    fn lamp_write_sends_no_neighbor_updates() {
        let mut lv = bench();
        let second = LAMP.offset(Dir::East);
        put(&mut lv, second, block::REDSTONE_LAMP.default_state());
        toggle(&mut lv);
        assert_eq!(at(&lv, LAMP), "redstone_lamp[lit=true]");
        assert_eq!(at(&lv, second), "redstone_lamp[lit=false]");
        assert!(
            lv.rules
                .nc_log
                .iter()
                .all(|(_, src)| *src != block::REDSTONE_LAMP)
        );
    }

    // 設置の時の状態は、受電で決まる (V11 §4.1)。
    #[test]
    fn placement_state_follows_power() {
        let mut lv = bench();
        let spot = LEVER.offset(Dir::South);
        assert_eq!(
            placement_state(&lv, spot).to_string(),
            "redstone_lamp[lit=false]"
        );
        toggle(&mut lv);
        assert_eq!(
            placement_state(&lv, spot).to_string(),
            "redstone_lamp[lit=true]"
        );
    }
}
