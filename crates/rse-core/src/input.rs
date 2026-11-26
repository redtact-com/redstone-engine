//! tick の外の入力 (V01 §2.4・§4.10・19 §4.1)。
//!
//! 入力は tick の関数の外で受ける。呼ばれたその場で、連鎖を最後まで回し切る
//! (更新器が最外で回し切るので、戻った時には終わっている。V04 §7.5)。
//!
//! - fixture の規約は「state[t] は tick t の全相の後、tick t の入力の直後」。
//!   tick を 1 回呼び、その tick の入力を呼び、状態を読む。
//! - 予約の基準の時刻は今のゲーム時刻。入力由来の予約は、tick の中の予約より 1 tick 早く見える (V01 §2.4)。
//! - 凍結中も入力は通る。予約とブロックイベントは溜まるだけ (V01 §4.8)。
//!
//! 縦切りで中身があるのは「使う」だけ。ほかの入力は席で、UNIMPLEMENTED を返す。

use crate::level::Level;
use crate::rules::Rules;
use crate::state::StateId;
use crate::status::Status;
use rse_java::Pos;

/// 入力の区間の印 (19 §4.1)。tick N の相が全部終わってから tick N+1 のゲーム時刻が進むまでの間で、
/// 実機が入力を処理する場所を表す。宣言の順が、境界の中での時刻順 (V01 §2.4 の 2〜5)。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum InputSection {
    /// プレイヤーの移動に伴う検知 (プレイヤーの在席)。
    B5 = 0,
    /// コマンド (setblock)。
    C = 1,
    /// プレイヤーの操作 (右クリック・設置)。
    A = 2,
    /// 関数 (今は使わない)。
    B1 = 3,
}

/// 入力 1 件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    /// 使う (右クリック)。レバーの切り替えなど (V01 §4.10)。
    Use { pos: Pos },
    /// setblock。席だけ。
    /// 経路はふつうの書き込みと順が違う (V04 §4.11。設置フック → SU → 撤去フック → NC → CU)。
    SetBlock {
        pos: Pos,
        state: StateId,
        strict: bool,
    },
}

/// 区間の印を付けた入力。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkedInput {
    pub section: InputSection,
    pub input: Input,
}

/// 同じ tick の入力を、区間の順に安定に並べる (19 §4.1。fixture の実行器が使う)。
/// 同じ区間の中は、元の並びを保つ。アプリの対話の入力は並べ替えず、呼んだ順に適用する。
///
/// UNVERIFIED: コマンド (C) とプレイヤーの操作 (A) の前後は [要実機] のまま (19 §4.1)。
/// ここでは V01 §2.4 の時刻順 (C が先) に並べる。
pub fn sort_by_section(inputs: &mut [MarkedInput]) {
    inputs.sort_by_key(|i| i.section);
}

/// 入力の結果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputOutcome {
    /// 素子が入力を受けて何かしたか。
    pub accepted: bool,
    /// この入力の間 (と、前に取り出した後) に立った状態コード。
    pub status: Status,
}

impl<R: Rules> Level<R> {
    /// 入力を 1 件適用する。戻った時点で、近隣更新の連鎖は全部終わっている。
    ///
    /// 区間の印は順を変えない (呼んだ順に適用する)。trace を入れる時に、事象へ付ける。
    pub fn input(&mut self, section: InputSection, input: Input) -> InputOutcome {
        let _ = section;
        let accepted = match input {
            Input::Use { pos } => {
                let state = self.world.get(pos);
                R::use_block(self, pos, state)
            }
            Input::SetBlock { .. } => {
                self.raise(Status::UNIMPLEMENTED);
                false
            }
        };
        InputOutcome {
            accepted,
            status: self.take_status(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InputSection::*;
    use super::*;
    use crate::set_block::UpdateFlags;
    use crate::state::block;
    use crate::testing::{Act, Ev, level, lit_lamp};
    use crate::ticks::TickPriority;

    // 境界の中での時刻順 (V01 §2.4): 移動に伴う検知 → コマンド → プレイヤーの操作 → 関数。
    #[test]
    fn section_order_is_fixed() {
        let mut all = [A, B1, C, B5];
        all.sort();
        assert_eq!(all, [B5, C, A, B1]);
        assert_eq!(all.map(|s| s as u8), [0, 1, 2, 3]);
    }

    // 同じ区間の中は、元の並びを保つ (安定)。
    #[test]
    fn sort_is_stable_within_a_section() {
        let at = |x| Input::Use {
            pos: Pos::new(x, 0, 0),
        };
        let mut inputs = [
            MarkedInput {
                section: A,
                input: at(0),
            },
            MarkedInput {
                section: C,
                input: at(1),
            },
            MarkedInput {
                section: A,
                input: at(2),
            },
            MarkedInput {
                section: C,
                input: at(3),
            },
        ];
        sort_by_section(&mut inputs);
        let order: Vec<Input> = inputs.iter().map(|i| i.input).collect();
        assert_eq!(order, [at(1), at(3), at(0), at(2)]);
    }

    // 入力は、その場で連鎖を回し切る。戻った時には、書き込みも近隣更新も終わっている。
    #[test]
    fn input_runs_the_whole_chain_before_returning() {
        let mut lv = level();
        let lever = Pos::new(0, 0, 0);
        let lamp = Pos::new(1, 0, 0);
        lv.set_state_raw(lever, block::LEVER.default_state());
        lv.set_state_raw(lamp, block::REDSTONE_LAMP.default_state());
        // 「使う」でレバーの隣へ NC を配り、NC を受けたランプが自分を点ける。
        lv.rules
            .on_use
            .insert(lever, vec![Act::UpdateNeighbors(lever, block::LEVER)]);
        lv.rules
            .on_nc
            .insert(lamp, vec![Act::Set(lamp, lit_lamp(), UpdateFlags::CLIENTS)]);
        let out = lv.input(A, Input::Use { pos: lever });
        assert_eq!(
            out,
            InputOutcome {
                accepted: true,
                status: Status::OK
            }
        );
        assert_eq!(lv.get_state(lamp), lit_lamp());
        assert_eq!(lv.game_time(), 0);
        // 何もしない所を使っても、受け付けない。
        let out = lv.input(A, Input::Use { pos: lamp });
        assert!(!out.accepted);
    }

    // 入力由来の予約は、tick の中で積んだ予約より 1 tick 早く見える (V01 §2.4)。
    // tick 1 の後の入力で遅延 2 を積むと tick 3 に走る。tick 2 の中で遅延 2 を積むと tick 4 に走る。
    #[test]
    fn input_ticks_look_one_tick_early() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP;
        let (a, b) = (Pos::new(0, 0, 0), Pos::new(1, 0, 0));
        lv.set_state_raw(a, lamp.default_state());
        lv.set_state_raw(b, lamp.default_state());
        lv.rules
            .on_use
            .insert(a, vec![Act::Schedule(a, lamp, 2, TickPriority::Normal)]);
        // b は tick 2 に走り、その中で遅延 2 を積み直す。
        lv.schedule_tick(b, lamp, 2, TickPriority::Normal);
        lv.rules
            .on_tick
            .insert(b, vec![Act::Schedule(b, lamp, 2, TickPriority::Normal)]);
        lv.tick();
        lv.input(A, Input::Use { pos: a });
        for _ in 0..3 {
            lv.tick();
        }
        let fired: Vec<(i32, i64)> = lv
            .rules
            .log
            .iter()
            .filter_map(|e| match e {
                Ev::Tick { pos, time, .. } => Some((pos.x, *time)),
                _ => None,
            })
            .collect();
        assert_eq!(fired, [(1, 2), (0, 3), (1, 4)]);
    }

    // 凍結中も入力は通り、連鎖はその場で終わる。予約は溜まるだけ (V01 §4.8)。
    #[test]
    fn input_works_while_frozen() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP;
        let a = Pos::new(0, 0, 0);
        lv.set_state_raw(a, lamp.default_state());
        lv.rules.on_use.insert(
            a,
            vec![
                Act::Set(a, lit_lamp(), UpdateFlags::ALL),
                Act::Schedule(a, lamp, 1, TickPriority::Normal),
            ],
        );
        lv.tick_frozen();
        assert!(lv.input(A, Input::Use { pos: a }).accepted);
        assert_eq!(lv.get_state(a), lit_lamp());
        lv.tick_frozen();
        assert_eq!(lv.scheduled_tick_count(), 1);
        assert!(!lv.rules.log.iter().any(|e| matches!(e, Ev::Tick { .. })));
    }

    // 席だけの入力は、黙って捨てずに UNIMPLEMENTED を返す。
    #[test]
    fn unimplemented_inputs_say_so() {
        let mut lv = level();
        let out = lv.input(
            C,
            Input::SetBlock {
                pos: Pos::new(0, 0, 0),
                state: block::STONE.default_state(),
                strict: false,
            },
        );
        assert_eq!(
            out,
            InputOutcome {
                accepted: false,
                status: Status::UNIMPLEMENTED
            }
        );
        assert_eq!(lv.get_state(Pos::new(0, 0, 0)), StateId::AIR);
    }
}
