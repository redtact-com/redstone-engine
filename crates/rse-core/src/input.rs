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
//! 中身があるのは「使う」「置く」「壊す」。setblock は席で、UNIMPLEMENTED を返す。
//!
//! # 置く (プレイヤーの設置。V04 §4.13)
//!
//! 確かめを全部済ませてから、1 回だけ書く。確かめの間は世界を書かず、フックも呼ばない。
//! この節で「手順 N」とだけ書いた物は、V04 §4.13 の BlockItem.place の手順の番号。書き込み (Level.setBlock) の
//! 中の手順は「書き込みの手順 N」(V04 §4.1) と書く。
//!
//! 1. 置く先の今の状態が置き換え可能か (手順 2。V05 §4.7.1。縦切りの表では空気だけ)。
//! 2. 置いた時の状態を決める (手順 4。[`Rules::placement_state`])。
//! 3. その状態が置く先に居られるか (手順 5。[`Rules::can_survive`])。
//! 4. フラグ 11 で書く (手順 7)。
//!
//! - 手順 6 (当たり判定) は省く。エンティティを持たないエンジンでは、いつも通る。
//! - 手順 1 (有効な機能)・3 (文脈の差し替え)・10 (音と GameEvent)・11 (アイテムの消費) は持たない。
//!   入力は置く先のマスと置く状態を運ぶので、押したマスと面から置く先を決める段 (BlockPlaceContext) も持たない。
//! - 手順 2〜5 は、置く先にまだ古いブロックがある世界で決まる (V04 §4.13 の注・V11 §4.1)。
//! - 書き込みの順 (古いブロックの撤去フック → 設置フック → NC → CU → 古い状態の間接 SU → 6 方向の SU →
//!   新しい状態の間接 SU。V04 §4.13 の「手順 7 の書き込みの中で起きること」・V04 §7.2 の 1) は
//!   [`crate::set_block`] のふつうの手順のまま。設置フックの中で状態が変わると外側の NC と SU が出ない
//!   (V06 §4.14 の落とし穴) のも、書き込みの手順 5 (読み直し) が受け持つ。
//! - 手順 4 の状態は、手順 7 でそのまま書かれる。周りの形状に合わせて直す段は通らない (V04 §4.13 の注)。
//!   だから、置いた時の状態は 2 の口だけで決まる。
//! - /setblock の経路 (V04 §4.11) とは別物である。あちらは書く前に周りに合わせて直し、フラグ 2 + 256 で書き、
//!   後から NC を配る。孤立したダストは、設置では十字に、/setblock では点になる (V06 §4.14.1)。
//! - 置いた後の手順 8・9 (読み直し・BE の CU・Block.setPlacedBy) は、まだ無い。縦切りの 3 素子と石は
//!   setPlacedBy も BE も持たない (V04 §4.13 の表)。足す時は 4 の後に置き、書き込みが真を返した時だけ、
//!   置いた位置を読み直す。読み直した状態が置いたのと同じ種類のブロックなら、その状態を渡して呼ぶ
//!   (違えば手順 9 を丸ごと飛ばす)。ダイオードの族 (V07 §4.13) と一緒に、Rules の口として足す。
//!
//! 範囲の外の扱い:
//!
//! - 高さの外も、読むと空気 (置き換え可能) なので、1 を通って 2 の口が呼ばれる。その先は種類で分かれる。
//!   - 規則のある種類: 2 と 3 の口を呼んでから、4 で書き込みの手順 1 (高さの確かめ。V04 §4.1) が、何もせずに
//!     偽を返す。状態コードは立たない。
//!   - 規則の無い種類: 2 の口の答えで UNIMPLEMENTED を立てて戻る。3 の口も書き込みも通らない (高さの中と同じ)。
//! - 実機も同じ順で、範囲の外は void_air (置き換え可能) と読まれ、置く状態と支えを決めてから書き込みで断る
//!   (V04 §4.13 の「書き込みが偽を返した時」)。高さの上の外も下の外も同じである。
//! - 格子の枠の外は、4 の書き込みまで進んで SAFETY が立つ (実機なら書ける)。wasm の窓口は、置く前に断る。
//!
//! # 壊す (プレイヤーの破壊。V04 §4.14)
//!
//! 空気をフラグ 3 で書く ([`Level::remove_block`]。V04 §4.14 の手順 7・§4.10)。
//! 撤去フックは書き込みの中で走る (V04 §4.2 の手順 7。ダストは V06 §4.15、入ったレバーは V09 §4.1)。
//! NC の送り元は壊したブロックである。
//!
//! - V04 §4.14 の手順 6 (playerWillDestroy)・8 (destroy)・11 (playerDestroy) は持たない。縦切りの 3 素子と石は、どれも
//!   既定 (GameEvent・パーティクル・落とし物だけ) で、ブロックの書き込みは手順 7 の 1 回だけである
//!   (V04 §4.14 の表)。上書きする素子 (紐・TNT・ドアほか) を入れる時に、口として足す。
//! - 入口の確かめ (操作距離・クリエイティブとサバイバルの違い) も持たない。3 素子と石では、書き込みはどちらも同じ
//!   (V04 §4.14 の「クリエイティブとサバイバルの違い」)。ドロップ・GameEvent も持たない。
//! - 範囲は確かめない。どちらの外でも何も起きず、SAFETY も立たない。理由は外ごとに違う。
//!   - 高さの外: 書き込みの手順 1 (高さの確かめ。V04 §4.1) が、何もせずに偽を返す。
//!   - 格子の枠の外: 読むと空気なので、空気を壊すのは同じ状態の書き込みになる。書き込みは、同じ状態なら
//!     格子に触る前に戻る (V04 §5.1)。
//! - 空気を壊しても何も起きない (V04 §4.14 の「空気を壊そうとした時」)。このエンジンの空気は air だけである。

use crate::level::Level;
use crate::rules::{Placement, Rules};
use crate::set_block::UpdateFlags;
use crate::state::{StateId, flag};
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
    /// プレイヤーの操作 (右クリック・設置・破壊)。
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
    /// 置く (プレイヤーの設置。V01 §4.10・V04 §4.13)。
    /// `state` は置くブロックと、プレイヤーの操作で決まる値を運ぶ。周りで決まる値は、置いた時の規則が決める。
    Place { pos: Pos, state: StateId },
    /// 壊す (プレイヤーの破壊。V01 §4.10・V04 §4.14)。
    Break { pos: Pos },
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
    /// 素子が入力を受けて何かしたか。置くなら「書けた」、壊すなら「空気でない物を消した」。
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
            Input::Place { pos, state } => self.place(pos, state),
            // 範囲は確かめない (冒頭の注)。
            Input::Break { pos } => self.remove_block(pos, false),
        };
        InputOutcome {
            accepted,
            status: self.take_status(),
        }
    }

    /// 置く (冒頭の注の 1〜4)。書けたら真。
    fn place(&mut self, pos: Pos, requested: StateId) -> bool {
        // 2・3 の口には `&Level` だけを渡す。1〜3 の確かめの間に骨格が書くのは状態コード (UNIMPLEMENTED) だけで、
        // 断る時に、世界にもフックにも触れない。
        let Some(state) = self.placement_for(pos, requested) else {
            return false;
        };
        // 4. 高さの外は、ここで書き込みの手順 1 が断る (2・3 の口はもう呼んである。冒頭の「範囲の外の扱い」)。
        // 格子の枠の外は SAFETY が立つ (実機なら書ける)。
        self.set_block(pos, state, UpdateFlags::ALL_IMMEDIATE)
    }

    /// 置く前の確かめ。置く状態を返す。置かないなら `None`。
    fn placement_for(&mut self, pos: Pos, requested: StateId) -> Option<StateId> {
        let lv = &*self;
        if !lv.world.get(pos).has(flag::REPLACEABLE) {
            return None;
        }
        let state = match R::placement_state(lv, pos, requested) {
            Placement::State(state) => state,
            Placement::Refused => return None,
            Placement::Unimplemented => {
                self.raise(Status::UNIMPLEMENTED);
                return None;
            }
        };
        R::can_survive(lv, pos, state).then_some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::InputSection::*;
    use super::*;
    use crate::set_block::UpdateFlags;
    use crate::state::block;
    use crate::testing::{Act, Asked, Ev, level, lit_lamp};
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

    const SPOT: Pos = Pos::new(0, 0, 0);

    fn place(state: StateId) -> Input {
        Input::Place { pos: SPOT, state }
    }

    // 置く: 置いた時の状態 → 支えの順に聞いてから、1 回だけフラグ 11 で書く。聞く間は、フックも更新も 1 つも無い。
    // 置いた時の状態の口には、要求の状態がプロパティごとそのまま渡る (既定でない状態を要求し、骨格が既定の状態に
    // 直して渡していないことを見る)。支えの口には、置いた時の状態の口が返した状態が渡る (要求の状態ではない)。
    // 書き込みはふつうの順 (V04 §7.2 の 1): 撤去フック (空気) → 設置フック → NC 6 件 (送り元は古いブロック = 空気。
    // V15 §5.5) → 古い状態の間接 SU → SU 6 件 → 新しい状態の間接 SU。間接 SU に渡るフラグは 11 から 1 を落とした 10。
    #[test]
    fn place_asks_then_writes_once_with_flag_11() {
        let mut lv = level();
        // 要求は既定でない状態 (点いたランプ)、口の答えはそれと違う状態 (消えたランプ = 既定の状態)。
        let requested = lit_lamp();
        let lamp = block::REDSTONE_LAMP.default_state();
        assert_ne!(requested, lamp);
        lv.rules.placement.insert(SPOT, Placement::State(lamp));
        let out = lv.input(A, place(requested));
        assert_eq!(
            out,
            InputOutcome {
                accepted: true,
                status: Status::OK
            }
        );
        assert_eq!(lv.get_state(SPOT), lamp);
        assert_eq!(
            *lv.rules.asked.borrow(),
            [
                Asked::Placement {
                    pos: SPOT,
                    requested,
                    hooks_before: 0
                },
                Asked::Survive {
                    pos: SPOT,
                    state: lamp,
                    hooks_before: 0
                },
            ]
        );
        let log = &lv.rules.log;
        assert_eq!(
            log[..2],
            [
                Ev::Remove {
                    pos: SPOT,
                    old: StateId::AIR,
                    moved: false
                },
                Ev::Place {
                    pos: SPOT,
                    state: lamp,
                    old: StateId::AIR,
                    moved: false
                },
            ]
        );
        let ncs: Vec<&Ev> = log.iter().filter(|e| matches!(e, Ev::Nc { .. })).collect();
        assert_eq!(ncs.len(), 6);
        assert!(
            ncs.iter()
                .all(|e| matches!(e, Ev::Nc { src, .. } if *src == block::AIR))
        );
        let indirect: Vec<(StateId, UpdateFlags, i32)> = log
            .iter()
            .filter_map(|e| match e {
                Ev::Indirect {
                    state, flags, left, ..
                } => Some((*state, *flags, *left)),
                _ => None,
            })
            .collect();
        assert_eq!(
            indirect,
            [
                (StateId::AIR, UpdateFlags(10), 511),
                (lamp, UpdateFlags(10), 511)
            ]
        );
        assert_eq!(log.iter().filter(|e| matches!(e, Ev::Su { .. })).count(), 6);
        // 2 + 6 + 1 + 6 + 1。書き込みは 1 回だけ。
        assert_eq!(log.len(), 16);
    }

    // 断る 4 通り: 置き換え可能でない・置けない・規則が無い・支えが無い。
    // どれも世界は変わらず、フックも更新も 1 つも無い。状態コードが立つのは「規則が無い」だけ。
    // 置き換え可能でない時は、口も呼ばない。置けない・規則が無い時は、支えを聞かない。
    #[test]
    fn refused_place_writes_nothing_and_calls_no_hook() {
        let stone = block::STONE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        // (置く先の状態, 置いた時の状態の答え, 支えの答え, 口を呼ぶ数, 状態コード)
        type Case = (StateId, Option<Placement>, bool, usize, Status);
        let cases: [Case; 4] = [
            (stone, None, true, 0, Status::OK),
            (StateId::AIR, Some(Placement::Refused), true, 1, Status::OK),
            (
                StateId::AIR,
                Some(Placement::Unimplemented),
                true,
                1,
                Status::UNIMPLEMENTED,
            ),
            (StateId::AIR, None, false, 2, Status::OK),
        ];
        for (before, answer, survives, asked, status) in cases {
            let mut lv = level();
            lv.set_state_raw(SPOT, before);
            lv.drain_changes();
            if let Some(answer) = answer {
                lv.rules.placement.insert(SPOT, answer);
            }
            lv.rules.survive.insert(SPOT, survives);
            let out = lv.input(A, place(lamp));
            let case = format!("{before} {answer:?} {survives}");
            assert_eq!(
                out,
                InputOutcome {
                    accepted: false,
                    status
                },
                "{case}"
            );
            assert_eq!(lv.get_state(SPOT), before, "{case}");
            assert_eq!(lv.rules.log, [], "{case}");
            assert_eq!(lv.rules.asked.borrow().len(), asked, "{case}");
            assert!(lv.drain_changes().is_empty(), "{case}");
            assert_eq!(lv.scheduled_tick_count(), 0, "{case}");
        }
    }

    // 置く・壊すは、凍結中も通る。ゲーム時刻は進めない (V01 §4.8・§2.4)。
    #[test]
    fn place_and_break_work_while_frozen_and_keep_the_clock() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP.default_state();
        lv.tick();
        lv.tick_frozen();
        assert!(lv.input(A, place(lamp)).accepted);
        assert_eq!(lv.get_state(SPOT), lamp);
        assert!(lv.input(A, Input::Break { pos: SPOT }).accepted);
        assert_eq!(lv.get_state(SPOT), StateId::AIR);
        assert_eq!(lv.game_time(), 1);
        assert_eq!(lv.scheduled_tick_count(), 0);
    }

    // 設置フックが積んだ予約は、入力の時刻が基準になる (V01 §2.4)。tick 1 の後に置いて遅延 2 なら、tick 3 に走る。
    #[test]
    fn ticks_scheduled_while_placing_use_the_input_time() {
        let mut lv = level();
        let lamp = block::REDSTONE_LAMP;
        lv.rules.on_place.insert(
            SPOT,
            vec![Act::Schedule(SPOT, lamp, 2, TickPriority::Normal)],
        );
        lv.tick();
        lv.input(A, place(lamp.default_state()));
        for _ in 0..2 {
            lv.tick();
        }
        let fired: Vec<i64> = lv
            .rules
            .log
            .iter()
            .filter_map(|e| match e {
                Ev::Tick { time, .. } => Some(*time),
                _ => None,
            })
            .collect();
        assert_eq!(fired, [3]);
    }

    // 壊す: 空気をフラグ 3 で書く (V04 §4.10)。撤去フックは書き込みの中で、NC より先に走る (V04 §4.2 の手順 7)。
    // NC の送り元は壊したブロック (V15 §5.5)。間接 SU は古い状態 → 新しい状態の 2 回で、渡るフラグは 3 から 1 を
    // 落とした 2、残りの深さは 511 (最初の書き込みは 512 から 1 減らして渡す。V04 §4.7)。
    #[test]
    fn break_writes_air_with_flag_3() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        lv.set_state_raw(SPOT, stone);
        let out = lv.input(A, Input::Break { pos: SPOT });
        assert_eq!(
            out,
            InputOutcome {
                accepted: true,
                status: Status::OK
            }
        );
        assert_eq!(lv.get_state(SPOT), StateId::AIR);
        let log = &lv.rules.log;
        assert_eq!(
            log[0],
            Ev::Remove {
                pos: SPOT,
                old: stone,
                moved: false
            }
        );
        assert!(matches!(log[2], Ev::Nc { src, .. } if src == block::STONE));
        assert_eq!(log.iter().filter(|e| matches!(e, Ev::Nc { .. })).count(), 6);
        let indirect: Vec<(StateId, UpdateFlags, i32)> = log
            .iter()
            .filter_map(|e| match e {
                Ev::Indirect {
                    state, flags, left, ..
                } => Some((*state, *flags, *left)),
                _ => None,
            })
            .collect();
        assert_eq!(
            indirect,
            [
                (stone, UpdateFlags(2), 511),
                (StateId::AIR, UpdateFlags(2), 511)
            ]
        );
        assert_eq!(log.iter().filter(|e| matches!(e, Ev::Su { .. })).count(), 6);
        // 置いた時の状態と支えの口は、壊すでは呼ばない。
        assert!(lv.rules.asked.borrow().is_empty());
    }

    // 空気を壊しても何も起きない。範囲も確かめない。高さの外は書き込みの手順 1 が断り、格子の枠の外は
    // 読むと空気なので、同じ状態の書き込みが格子に触る前に戻る (V04 §5.1)。どちらも SAFETY は立たず、格子も広がらない。
    #[test]
    fn breaking_air_does_nothing_anywhere() {
        let mut lv = level();
        let stone = block::STONE.default_state();
        lv.set_state_raw(SPOT, stone);
        lv.drain_changes();
        // 格子の枠 (1 辺 256 セクション) の外。ここへ石を置く書き込みなら SAFETY が立つ。
        let far = Pos::new(16 * 256, 0, 0);
        assert!(!lv.can_set_all_raw([(far, stone)]));
        for pos in [
            Pos::new(3, 3, 3),
            Pos::new(0, 320, 0),
            Pos::new(0, -65, 0),
            far,
            Pos::new(i32::MAX, i32::MAX, i32::MIN),
        ] {
            let out = lv.input(A, Input::Break { pos });
            assert_eq!(
                out,
                InputOutcome {
                    accepted: false,
                    status: Status::OK
                },
                "{pos:?}"
            );
        }
        assert_eq!(lv.rules.log, []);
        assert!(lv.drain_changes().is_empty());
        assert_eq!(lv.get_state(SPOT), stone);
        // 対になる事実: 同じ枠の外へ置くと、書き込みまで進んで SAFETY が立つ (wasm の窓口は、置く前に断る)。
        let out = lv.input(
            A,
            Input::Place {
                pos: far,
                state: stone,
            },
        );
        assert_eq!(
            out,
            InputOutcome {
                accepted: false,
                status: Status::SAFETY
            }
        );
        // 高さの外へ置くのは、上の外も下の外も、書き込みの手順 1 が断る。読むと空気 (置き換え可能) なので、
        // 規則のある種類は、その前に 2 つの口を呼ぶ (実機も、置く状態と支えを決めてから書き込みで断る。V04 §4.13)。
        // 状態コードは立たない。規則の無い種類は、置いた時の状態の口の答えで UNIMPLEMENTED を立てて戻り、
        // 支えの口も書き込みも通らない。
        for y in [320, -65] {
            let out_of_height = Pos::new(0, y, 0);
            let place_stone = Input::Place {
                pos: out_of_height,
                state: stone,
            };
            lv.rules.asked.borrow_mut().clear();
            let out = lv.input(A, place_stone);
            assert_eq!(
                out,
                InputOutcome {
                    accepted: false,
                    status: Status::OK
                },
                "y={y}"
            );
            assert_eq!(
                *lv.rules.asked.borrow(),
                [
                    Asked::Placement {
                        pos: out_of_height,
                        requested: stone,
                        hooks_before: 0
                    },
                    Asked::Survive {
                        pos: out_of_height,
                        state: stone,
                        hooks_before: 0
                    },
                ],
                "y={y}"
            );
            lv.rules
                .placement
                .insert(out_of_height, Placement::Unimplemented);
            lv.rules.asked.borrow_mut().clear();
            let out = lv.input(A, place_stone);
            assert_eq!(
                out,
                InputOutcome {
                    accepted: false,
                    status: Status::UNIMPLEMENTED
                },
                "y={y}"
            );
            assert_eq!(
                *lv.rules.asked.borrow(),
                [Asked::Placement {
                    pos: out_of_height,
                    requested: stone,
                    hooks_before: 0
                }],
                "y={y}"
            );
        }
        assert_eq!(lv.rules.log, []);
        assert!(lv.drain_changes().is_empty());
    }
}
