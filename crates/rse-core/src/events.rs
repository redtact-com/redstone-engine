//! ブロックイベント (V03・19 §3.5)。
//!
//! 「ある位置のブロックに、整数 2 個の合図を後で届ける」仕組み。ピストンが、動くのを
//! ブロックイベントの相まで遅らせるのに使う (V03 §2.1)。
//!
//! - 挿入順を保つ集合で持つ。鍵は (位置, ブロックの種類, A, B) の 4 つ組 (V03 §3.2)。
//! - 重複を比べる相手は「まだ取り出していない分」だけ。取り出す時に集合から消す。
//! - 取り出してから処理の関数を呼ぶ。処理中に積まれた分は同じ相で回る。上限は設けない (V03 §4.2)。
//! - tick をまたいで持ち続ける。相の外 (入力・後ろの相) で積まれた分は、次の tick の相で回る (V03 §2.3)。
//!
//! 中身のあるイベント (ピストン・音符ブロック) は、その素子の PR で入る。ここは枠だけ。

use crate::level::Level;
use crate::rules::Rules;
use crate::state::BlockId;
use crate::status::Status;
use alloc::collections::{BTreeSet, VecDeque};
use rse_java::Pos;

/// ブロックイベント 1 件 (V03 §3.2)。A と B は i32 のまま持つ。
/// 列挙に畳むと、想定の外の値の素通りが再現できなくなる (V03 §7.1)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockEvent {
    pub pos: Pos,
    pub block: BlockId,
    pub a: i32,
    pub b: i32,
}

type EventKey = (i32, i32, i32, BlockId, i32, i32);

impl BlockEvent {
    fn key(&self) -> EventKey {
        (
            self.pos.x, self.pos.y, self.pos.z, self.block, self.a, self.b,
        )
    }
}

/// ブロックイベントの列 (V03 §3.1)。
pub struct BlockEvents {
    queue: VecDeque<BlockEvent>,
    /// まだ取り出していない分の鍵。所属の判定にだけ使う。
    pending: BTreeSet<EventKey>,
}

impl BlockEvents {
    pub(crate) fn new() -> BlockEvents {
        BlockEvents {
            queue: VecDeque::new(),
            pending: BTreeSet::new(),
        }
    }

    /// 末尾へ積む。同じ 4 成分がまだ列にあれば何もしない (位置も順も変えない)。
    fn push(&mut self, event: BlockEvent) -> bool {
        if !self.pending.insert(event.key()) {
            return false;
        }
        self.queue.push_back(event);
        true
    }

    /// 先頭を取り出す。集合からも消すので、処理の最中に同じ 4 成分を積むと、新しい 1 件として末尾に入る。
    fn pop(&mut self) -> Option<BlockEvent> {
        let event = self.queue.pop_front()?;
        self.pending.remove(&event.key());
        Some(event)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl<R: Rules> Level<R> {
    /// ブロックイベントを積む (V03 §4.1。ServerLevel.blockEvent)。
    /// 判定は無い。その位置にそのブロックがあるか・凍結中かは見ない。積めたら真。
    pub fn block_event(&mut self, pos: Pos, block: BlockId, a: i32, b: i32) -> bool {
        self.events.push(BlockEvent { pos, block, a, b })
    }

    /// 待っているブロックイベントの数。
    pub fn pending_block_events(&self) -> usize {
        self.events.len()
    }

    /// ブロックイベントの相 (V03 §4.2。ServerLevel.runBlockEvents)。空になるまで、先頭から 1 件ずつ回す。
    ///
    /// 配送の時に、位置の今のブロックの種類を照合する。違えば捨てる。見るのは種類だけで、
    /// プロパティは見ない。処理の関数には今の状態を渡す (V03 §4.3)。
    /// 全域を tick するので、範囲の外への退避は持たない (V03 §7.3)。
    pub(crate) fn run_block_events(&mut self) {
        let mut delivered: u32 = 0;
        while let Some(event) = self.events.pop() {
            if let Some(limit) = self.config.block_event_safety
                && delivered >= limit
            {
                // 安全弁。実機には上限が無いので、ここで止めると実機と違う結果になる。
                // 取り出した 1 件を先頭へ戻し、残りは次の tick に回す。黙って捨てない (V03 §7.2 の 5)。
                self.events.pending.insert(event.key());
                self.events.queue.push_front(event);
                self.status |= Status::SAFETY;
                break;
            }
            delivered += 1;
            let state = self.world.get(event.pos);
            if state.block() == event.block {
                R::block_event(self, event.pos, state, event.a, event.b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Config;
    use crate::state::block;
    use crate::testing::{Act, Ev, level, level_with, lit_lamp};

    const LAMP: BlockId = block::REDSTONE_LAMP;

    fn lamp_at(lv: &mut Level<crate::testing::Probe>, x: i32) -> Pos {
        let pos = Pos::new(x, 0, 0);
        lv.set_state_raw(pos, LAMP.default_state());
        pos
    }

    fn delivered(lv: &Level<crate::testing::Probe>) -> Vec<(i32, i32, i32)> {
        lv.rules
            .log
            .iter()
            .filter_map(|e| match e {
                Ev::Event { pos, a, b, .. } => Some((pos.x, *a, *b)),
                _ => None,
            })
            .collect()
    }

    // 完全な先入れ先出し。優先度も、位置による並べ替えも無い (V03 §4.2)。
    #[test]
    fn runs_in_insertion_order() {
        let mut lv = level();
        for x in [5, 1, 9, 3] {
            let pos = lamp_at(&mut lv, x);
            assert!(lv.block_event(pos, LAMP, x, 0));
        }
        lv.tick();
        assert_eq!(delivered(&lv), [(5, 5, 0), (1, 1, 0), (9, 9, 0), (3, 3, 0)]);
        assert_eq!(lv.pending_block_events(), 0);
    }

    // 4 成分が全部同じ時だけ重複。重複の時、既にある要素の位置は動かない (V03 §3.2)。
    #[test]
    fn dedupes_on_all_four_components() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        assert!(lv.block_event(a, LAMP, 0, 2));
        assert!(lv.block_event(b, LAMP, 0, 2));
        assert!(!lv.block_event(a, LAMP, 0, 2)); // 重複。末尾へ移らない
        assert!(lv.block_event(a, LAMP, 1, 2)); // A が違う
        assert!(lv.block_event(a, LAMP, 0, 3)); // B が違う
        assert!(lv.block_event(a, block::LEVER, 0, 2)); // 種類が違う (配送で捨てられる)
        assert_eq!(lv.pending_block_events(), 5);
        lv.tick();
        assert_eq!(delivered(&lv), [(0, 0, 2), (1, 0, 2), (0, 1, 2), (0, 0, 3)]);
    }

    // 処理中に積まれたイベントは末尾に付き、同じ tick に回る。
    // 取り出し済みのイベントは集合に無いので、自分と同じ 4 成分も新しい 1 件として入る (V03 §3.2)。
    #[test]
    fn events_pushed_while_running_run_in_the_same_tick() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        lv.block_event(a, LAMP, 0, 0);
        lv.block_event(b, LAMP, 0, 0);
        lv.rules.on_event.insert(
            a,
            vec![Act::Event(a, LAMP, 0, 0), Act::Event(b, LAMP, 0, 0)],
        );
        lv.tick();
        // b は列に残っていたので重複。a は取り出し済みなので、末尾に新しく入る。
        assert_eq!(delivered(&lv), [(0, 0, 0), (1, 0, 0), (0, 0, 0)]);
    }

    // 配送の時に種類を照合する。違えば捨て、同じ種類ならプロパティが変わっていても今の状態で届く (V03 §4.3)。
    #[test]
    fn delivery_checks_block_kind_only() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        lv.block_event(a, LAMP, 0, 0);
        lv.block_event(b, LAMP, 0, 0);
        lv.set_state_raw(a, block::STONE.default_state());
        lv.set_state_raw(b, lit_lamp());
        lv.tick();
        assert_eq!(
            lv.rules.log,
            [Ev::Event {
                pos: b,
                state: lit_lamp(),
                a: 0,
                b: 0
            }]
        );
        assert_eq!(lv.pending_block_events(), 0);
    }

    // 予約 tick の相で積んだイベントは同じ tick に回る。tick の外で積んだ分は、次に来る tick で回る (V03 §2.3)。
    #[test]
    fn phase_position() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        lv.schedule_tick(a, LAMP, 1, crate::ticks::TickPriority::Normal);
        lv.rules.on_tick.insert(a, vec![Act::Event(a, LAMP, 7, 0)]);
        lv.tick();
        assert_eq!(delivered(&lv), [(0, 7, 0)]);
        lv.block_event(a, LAMP, 8, 0);
        assert_eq!(lv.pending_block_events(), 1);
        lv.tick();
        assert_eq!(delivered(&lv), [(0, 7, 0), (0, 8, 0)]);
    }

    // 安全弁は既定では無い。入れた時だけ働き、SAFETY を返して残りを次の tick に回す (V03 §7.2 の 5)。
    #[test]
    fn safety_valve_reports_and_keeps_the_rest() {
        let mut lv = level_with(Config {
            block_event_safety: Some(2),
            ..Config::default()
        });
        for x in 0..3 {
            let pos = lamp_at(&mut lv, x);
            lv.block_event(pos, LAMP, 0, 0);
        }
        assert_eq!(lv.tick(), Status::SAFETY);
        assert_eq!(delivered(&lv).len(), 2);
        assert_eq!(lv.pending_block_events(), 1);
        // 戻した 1 件は、集合にも戻っている。
        assert!(!lv.block_event(Pos::new(2, 0, 0), LAMP, 0, 0));
        assert_eq!(lv.tick(), Status::OK);
        assert_eq!(delivered(&lv).len(), 3);
    }
}
