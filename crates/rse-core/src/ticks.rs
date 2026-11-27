//! 予約 tick (V02・19 §3.4)。
//!
//! 実機と同じ 2 層にする。全体のキュー 1 本には畳まない (V02 §7.1)。
//! - チャンクごとの入れ物: (発火 tick, 優先度, 予約順) の昇順のキューと、(位置, 種類) の集合。
//! - 毎 tick、期限の来た予約を**先に全部集めてから**順に実行する。
//!   集める順は、各チャンクの先頭どうしを (優先度, 予約順) で比べて併合した順 (V02 §4.3)。
//! - 同じ (位置, 種類) の予約は、入れ物に 1 件しか入らない。後から来た方を捨てる (V02 §4.2)。
//!
//! 守る順序は V02 §7.2 の 8 項目。集合は所属の判定にだけ使い、反復しない。
//!
//! 流体用の一式と、読み込んだ予約 (負の予約順) の口は、使う PR で足す。型は種類 `K` で共有できる。

use crate::level::Level;
use crate::rules::Rules;
use crate::state::BlockId;
use crate::status::Status;
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use core::cmp::Reverse;
use rse_java::Pos;

/// 優先度 (V02 §3.2 の全値)。値が小さいほど先。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(i8)]
pub enum TickPriority {
    ExtremelyHigh = -3,
    VeryHigh = -2,
    High = -1,
    Normal = 0,
    Low = 1,
    VeryLow = 2,
    ExtremelyLow = 3,
}

impl TickPriority {
    /// 値から引く。一致が無い時、-3 より小さければ最高、それ以外は最低に丸める (V02 §3.2)。
    pub const fn by_value(value: i32) -> TickPriority {
        match value {
            -3 => TickPriority::ExtremelyHigh,
            -2 => TickPriority::VeryHigh,
            -1 => TickPriority::High,
            0 => TickPriority::Normal,
            1 => TickPriority::Low,
            2 => TickPriority::VeryLow,
            3 => TickPriority::ExtremelyLow,
            v if v < -3 => TickPriority::ExtremelyHigh,
            _ => TickPriority::ExtremelyLow,
        }
    }
}

/// 予約 1 件 (V02 §3.1)。「何をするか」は持たない。発火の時の世界から決まる。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScheduledTick<K> {
    /// 予約した種類。状態ではなく、ブロックの同一性。
    pub kind: K,
    pub pos: Pos,
    /// 発火するゲーム時刻。
    pub trigger: i64,
    pub priority: TickPriority,
    /// 予約順。世界ごとの通し番号。
    pub sub: i64,
}

type PosKey = (i32, i32, i32);

#[inline]
fn pos_key(pos: Pos) -> PosKey {
    (pos.x, pos.y, pos.z)
}

/// キューの鍵。発火 tick → 優先度 → 予約順 (V02 §2.4)。
/// 残りの成分は予約の中身を運ぶだけで、順には効かない (1 つの入れ物の中で予約順は重ならない)。
type QueueItem<K> = Reverse<(i64, TickPriority, i64, PosKey, K)>;

/// 集める段の候補。チャンクの先頭の (優先度, 予約順) と、チャンクの座標。
type DueChunk = Reverse<(TickPriority, i64, (i32, i32))>;

/// チャンク 1 個分の入れ物 (V02 §3.3)。
struct ChunkTicks<K> {
    queue: BinaryHeap<QueueItem<K>>,
    /// 入れ物にある (位置, 種類)。重複の抑止と「入れ物にあるか」の問い合わせに使う。
    present: BTreeSet<(PosKey, K)>,
}

impl<K: Copy + Ord> ChunkTicks<K> {
    fn new() -> ChunkTicks<K> {
        ChunkTicks {
            queue: BinaryHeap::new(),
            present: BTreeSet::new(),
        }
    }

    /// 先頭の (発火 tick, 優先度, 予約順)。
    fn peek(&self) -> Option<(i64, TickPriority, i64)> {
        self.queue
            .peek()
            .map(|Reverse((trigger, priority, sub, _, _))| (*trigger, *priority, *sub))
    }

    /// 先頭を取り出す。集合から外すのは、この取り出しの時 (V02 §7.2 の 1)。
    fn poll(&mut self) -> Option<ScheduledTick<K>> {
        let Reverse((trigger, priority, sub, key, kind)) = self.queue.pop()?;
        self.present.remove(&(key, kind));
        Some(ScheduledTick {
            kind,
            pos: Pos::new(key.0, key.1, key.2),
            trigger,
            priority,
            sub,
        })
    }
}

/// 世界 1 個分 (V02 §3.4)。ブロック用と流体用で 1 つずつ持つ形。
pub struct LevelTicks<K> {
    /// チャンク → 入れ物。空になった入れ物は消す。
    chunks: BTreeMap<(i32, i32), ChunkTicks<K>>,
    /// 今 tick に実行する予約の列 (先入れ先出し)。
    run_now: VecDeque<ScheduledTick<K>>,
    /// 今 tick の未実行分の (位置, 種類)。
    run_now_set: BTreeSet<(PosKey, K)>,
}

impl<K: Copy + Ord> LevelTicks<K> {
    pub(crate) fn new() -> LevelTicks<K> {
        LevelTicks {
            chunks: BTreeMap::new(),
            run_now: VecDeque::new(),
            run_now_set: BTreeSet::new(),
        }
    }

    /// 予約を入れる。同じ (位置, 種類) が入れ物にあれば捨てて偽を返す (V02 §4.2)。
    /// 既存の予約の発火 tick と優先度は変わらない (先勝ち。V02 §5.3)。
    pub(crate) fn schedule(&mut self, tick: ScheduledTick<K>) -> bool {
        let key = pos_key(tick.pos);
        let chunk = self
            .chunks
            .entry(tick.pos.chunk())
            .or_insert_with(ChunkTicks::new);
        if !chunk.present.insert((key, tick.kind)) {
            return false;
        }
        chunk.queue.push(Reverse((
            tick.trigger,
            tick.priority,
            tick.sub,
            key,
            tick.kind,
        )));
        true
    }

    /// 入れ物にあるか。予約が入ってから、集める段で取り出されるまで真 (V02 §4.5)。
    pub fn has_scheduled_tick(&self, pos: Pos, kind: K) -> bool {
        self.chunks
            .get(&pos.chunk())
            .is_some_and(|c| c.present.contains(&(pos_key(pos), kind)))
    }

    /// 今 tick の未実行分にあるか。集める段で取り出されてから、配送の直前まで真 (V02 §4.5)。
    /// 「入れ物にあるか」とは重ならない。合成した問い合わせは作らない (V02 §7.4)。
    pub fn will_tick_this_tick(&self, pos: Pos, kind: K) -> bool {
        self.run_now_set.contains(&(pos_key(pos), kind))
    }

    /// 入れ物にある予約の数 (今 tick 分に取り出した物は数えない)。
    pub fn scheduled_count(&self) -> usize {
        self.chunks.values().map(|c| c.queue.len()).sum()
    }

    /// 集める (V02 §4.3。LevelTicks.collectTicks)。発火 tick が `now` 以下の予約を、上限まで今 tick 分へ移す。
    ///
    /// 1. 先頭の発火 tick が `now` 以下のチャンクを候補にする。
    /// 2. 候補から、先頭の (優先度, 予約順) が最小のチャンクを取り、先頭を取り出す。
    /// 3. 同じチャンクから続けて取り出す。止めるのは、期限の来た先頭が無い時と、
    ///    先頭が次点のチャンクの先頭より**大きい**時 (等しければ続ける。V02 §7.2 の 6)。
    ///    次点の先頭は、ここで 1 回だけ読む。
    /// 4. チャンクに期限の来た先頭が残っていれば、新しい鍵で候補へ戻す。
    ///
    /// 同じチャンクの中では発火 tick が最優先なので、期限切れは優先度に関係なく先に出る (V02 §5.4)。
    pub(crate) fn collect(&mut self, now: i64, limit: usize) -> Status {
        let mut status = Status::OK;
        // 候補の鍵は (優先度, 予約順)。発火 tick は見ない (V02 §2.4)。
        // 3 つ目の成分 (チャンクの座標) は、同点の時にエンジンの中で順を決めるためだけにある。
        let mut due: BinaryHeap<DueChunk> = BinaryHeap::new();
        for (key, chunk) in &self.chunks {
            if let Some((trigger, priority, sub)) = chunk.peek()
                && trigger <= now
            {
                due.push(Reverse((priority, sub, *key)));
            }
        }
        let mut left_behind = false;
        while self.run_now.len() < limit {
            let Some(Reverse((priority, sub, key))) = due.pop() else {
                break;
            };
            let runner_up = due.peek().map(|Reverse((p, s, _))| (*p, *s));
            if runner_up == Some((priority, sub)) {
                // 別のチャンクの先頭と同点。実機の順はハッシュ表の反復順で決まり、ここでは写していない
                // (V02 §5.5・V18 の R4)。黙って進めず、コードを立てる。
                status |= Status::LOADED_ORDER_TIE;
            }
            let Some(chunk) = self.chunks.get_mut(&key) else {
                continue;
            };
            // 入れ物 (chunks) と今 tick 分の列は別のフィールドなので、取り出しながら直に足せる。
            if let Some(tick) = chunk.poll() {
                self.run_now_set.insert((pos_key(tick.pos), tick.kind));
                self.run_now.push_back(tick);
            }
            while self.run_now.len() < limit {
                let Some((trigger, p, s)) = chunk.peek() else {
                    break;
                };
                if trigger > now {
                    break;
                }
                if runner_up.is_some_and(|best| (p, s) > best) {
                    break;
                }
                if let Some(tick) = chunk.poll() {
                    self.run_now_set.insert((pos_key(tick.pos), tick.kind));
                    self.run_now.push_back(tick);
                }
            }
            match chunk.peek() {
                Some((trigger, p, s)) if trigger <= now => {
                    if self.run_now.len() < limit {
                        due.push(Reverse((p, s, key)));
                    } else {
                        left_behind = true;
                    }
                }
                Some(_) => {}
                None => {
                    self.chunks.remove(&key);
                }
            }
        }
        // 上限に達して残った分は、入れ物に置いたままにする (集合にも残る。V02 §7.2 の 7)。
        // 発火 tick は過去のままなので、次の tick に期限切れとして集まる。
        if left_behind || !due.is_empty() {
            status |= Status::TICK_CARRYOVER;
        }
        status
    }

    /// 今 tick 分の先頭を取り出す。未実行分の集合から外すのは、配送の直前のここ (V02 §7.2 の 3)。
    pub(crate) fn pop_run_now(&mut self) -> Option<ScheduledTick<K>> {
        let tick = self.run_now.pop_front()?;
        self.run_now_set.remove(&(pos_key(tick.pos), tick.kind));
        Some(tick)
    }
}

impl<R: Rules> Level<R> {
    /// ブロックの tick を予約する (V02 §4.1。LevelAccessor.scheduleTick)。
    ///
    /// 発火 tick は「今のゲーム時刻 + 遅延」。遅延は 0 でも負でも通るが、積んだ tick には走らない (V02 §5.1)。
    /// 予約順のカウンターは、重複で捨てる時も進める (V02 §3.6)。入ったら真。
    pub fn schedule_tick(
        &mut self,
        pos: Pos,
        kind: BlockId,
        delay: i32,
        priority: TickPriority,
    ) -> bool {
        let sub = self.sub_tick;
        self.sub_tick += 1;
        self.block_ticks.schedule(ScheduledTick {
            kind,
            pos,
            trigger: self.game_time + delay as i64,
            priority,
            sub,
        })
    }

    /// ブロックの予約が入れ物にあるか (V02 §4.5。hasScheduledTick)。
    pub fn has_scheduled_tick(&self, pos: Pos, kind: BlockId) -> bool {
        self.block_ticks.has_scheduled_tick(pos, kind)
    }

    /// ブロックの予約が今 tick の未実行分にあるか (V02 §4.5。willTickThisTick)。
    pub fn will_tick_this_tick(&self, pos: Pos, kind: BlockId) -> bool {
        self.block_ticks.will_tick_this_tick(pos, kind)
    }

    /// 入れ物にあるブロックの予約の数。
    pub fn scheduled_tick_count(&self) -> usize {
        self.block_ticks.scheduled_count()
    }

    /// ブロックの予約 tick の相 (V02 §2.3)。集める → 回す。
    ///
    /// 集め終えてから実行するので、実行中に積まれた予約は今 tick には走らない (V02 §7.2 の 2)。
    /// 配送の時に種類を照合する。照合は状態ではなく種類で、違えば空振りする (V02 §4.6)。
    pub(crate) fn run_block_ticks(&mut self) {
        let status = self
            .block_ticks
            .collect(self.game_time, self.config.max_ticks_per_tick);
        self.status |= status;
        while let Some(tick) = self.block_ticks.pop_run_now() {
            let state = self.world.get(tick.pos);
            if state.block() == tick.kind {
                R::scheduled_tick(self, tick.pos, state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TickPriority::*;
    use super::*;
    use crate::level::Config;
    use crate::state::block;
    use crate::testing::{Act, Ev, level, level_with};

    const LAMP: BlockId = block::REDSTONE_LAMP;

    /// x の列に並べたランプ。位置で予約を見分ける。
    fn lamp_at(lv: &mut Level<crate::testing::Probe>, x: i32) -> Pos {
        let pos = Pos::new(x, 0, 0);
        lv.set_state_raw(pos, LAMP.default_state());
        pos
    }

    /// 配送された予約の x 座標の列。
    fn ticked(lv: &Level<crate::testing::Probe>) -> Vec<i32> {
        lv.rules
            .log
            .iter()
            .filter_map(|e| match e {
                Ev::Tick { pos, .. } => Some(pos.x),
                _ => None,
            })
            .collect()
    }

    // 優先度の値 (V02 §3.2)。
    #[test]
    fn priority_values_are_fixed() {
        let all = [
            ExtremelyHigh,
            VeryHigh,
            High,
            Normal,
            Low,
            VeryLow,
            ExtremelyLow,
        ];
        assert_eq!(all.map(|p| p as i8), [-3, -2, -1, 0, 1, 2, 3]);
        assert!(all.windows(2).all(|w| w[0] < w[1]));
        for p in all {
            assert_eq!(TickPriority::by_value(p as i32), p);
        }
        assert_eq!(TickPriority::by_value(-9), ExtremelyHigh);
        assert_eq!(TickPriority::by_value(9), ExtremelyLow);
    }

    // 同じ tick の中は (優先度, 予約順) の昇順 (V02 §2.4)。
    #[test]
    fn same_tick_runs_by_priority_then_schedule_order() {
        let mut lv = level();
        for (x, priority) in [
            (0, Normal),
            (1, High),
            (2, Normal),
            (3, ExtremelyHigh),
            (4, VeryHigh),
            (5, High),
        ] {
            let pos = lamp_at(&mut lv, x);
            assert!(lv.schedule_tick(pos, LAMP, 2, priority));
        }
        lv.tick();
        assert_eq!(ticked(&lv), []);
        lv.tick();
        assert_eq!(ticked(&lv), [3, 4, 1, 5, 0, 2]);
        assert_eq!(lv.scheduled_tick_count(), 0);
    }

    // 遅延 d の予約は、積んだ時のゲーム時刻 + d の tick に走る (V02 §4.1)。
    // tick の外 (入力) で積んだ予約は、tick の中で積んだ予約より 1 tick 早く見える (V01 §2.4)。
    #[test]
    fn fires_at_game_time_plus_delay() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        lv.schedule_tick(a, LAMP, 4, Normal);
        for _ in 0..3 {
            lv.tick();
            assert_eq!(ticked(&lv), []);
        }
        lv.tick();
        assert_eq!(lv.game_time(), 4);
        assert_eq!(
            lv.rules.log,
            [Ev::Tick {
                pos: a,
                state: LAMP.default_state(),
                time: 4
            }]
        );
    }

    // 集め終えてから実行する。実行中に積んだ予約は、遅延 0 でも負でも今 tick には走らず、次の tick に走る (V02 §5.1)。
    #[test]
    fn ticks_scheduled_while_running_wait_for_next_tick() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        let c = lamp_at(&mut lv, 2);
        lv.schedule_tick(a, LAMP, 1, Normal);
        lv.rules.on_tick.insert(
            a,
            vec![
                Act::Schedule(b, LAMP, 0, Normal),
                Act::Schedule(c, LAMP, -5, Normal),
            ],
        );
        lv.tick();
        assert_eq!(ticked(&lv), [0]);
        assert!(lv.has_scheduled_tick(b, LAMP));
        lv.tick();
        // 期限切れどうしは、同じチャンクの中で発火 tick の小さい方が先 (V02 §5.4)。
        assert_eq!(ticked(&lv), [0, 2, 1]);
    }

    // 同じチャンクの中では、期限切れが優先度に関係なく先に出る (V02 §5.4 の 1)。
    #[test]
    fn overdue_goes_first_within_a_chunk() {
        let mut lv = level();
        let late = lamp_at(&mut lv, 0);
        let urgent = lamp_at(&mut lv, 1);
        lv.schedule_tick(late, LAMP, 0, Normal); // 発火 tick 0 (期限切れ)
        lv.schedule_tick(urgent, LAMP, 1, ExtremelyHigh); // 発火 tick 1
        lv.tick();
        assert_eq!(ticked(&lv), [0, 1]);
    }

    // チャンク間は先頭どうしを (優先度, 予約順) だけで比べる。発火 tick は見ない。
    // だから、先頭が「期限切れだが優先度 0」のチャンクの後ろにある優先度 -3 の予約は、
    // 他のチャンクの優先度 -1 の予約より後に回る (V02 §5.4 の 3)。
    #[test]
    fn overdue_head_blocks_its_chunk() {
        let mut lv = level();
        let late = lamp_at(&mut lv, 0);
        let urgent = lamp_at(&mut lv, 1);
        let other = lamp_at(&mut lv, 16); // 別のチャンク
        lv.schedule_tick(late, LAMP, 0, Normal);
        lv.schedule_tick(urgent, LAMP, 1, ExtremelyHigh);
        lv.schedule_tick(other, LAMP, 1, High);
        lv.tick();
        assert_eq!(ticked(&lv), [16, 0, 1]);
    }

    // チャンクをまたいでも、期限ちょうどの予約は (優先度, 予約順) の全順序で走る (V02 §2.4)。
    #[test]
    fn merges_across_chunks() {
        let mut lv = level();
        // 予約順に、チャンクを行き来して積む。
        for (x, priority) in [
            (0, Normal),
            (16, Normal),
            (1, High),
            (32, Normal),
            (17, High),
            (2, Normal),
            (-1, ExtremelyHigh),
        ] {
            let pos = lamp_at(&mut lv, x);
            lv.schedule_tick(pos, LAMP, 1, priority);
        }
        lv.tick();
        assert_eq!(ticked(&lv), [-1, 1, 17, 0, 16, 32, 2]);
        assert!(lv.take_status().is_ok());
    }

    // 同じ (位置, 種類) は入れ物に 1 件。後から来た方を捨て、既存の発火 tick と優先度は変わらない (V02 §4.2・§5.3)。
    // 捨てる時も予約順のカウンターは進む (V02 §3.6)。
    #[test]
    fn duplicate_is_dropped_first_wins() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        assert!(lv.schedule_tick(a, LAMP, 3, Normal));
        assert!(!lv.schedule_tick(a, LAMP, 1, ExtremelyHigh));
        assert_eq!(lv.sub_tick, 2);
        assert_eq!(lv.scheduled_tick_count(), 1);
        lv.tick();
        lv.tick();
        assert_eq!(ticked(&lv), []);
        lv.tick();
        assert_eq!(ticked(&lv), [0]);
        // 種類が違えば別の予約 (V02 §5.6)。
        assert!(lv.schedule_tick(a, LAMP, 1, Normal));
        assert!(lv.schedule_tick(a, block::LEVER, 1, Normal));
        assert_eq!(lv.scheduled_tick_count(), 2);
    }

    // 「入れ物にあるか」と「今 tick の未実行分にあるか」は重ならない (V02 §4.5)。
    // 取り出された予約は入れ物の集合から外れるので、実行の前でも同じ (位置, 種類) をもう 1 件積める (V02 §5.2)。
    #[test]
    fn two_queries_do_not_overlap() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        lv.schedule_tick(a, LAMP, 1, Normal);
        lv.schedule_tick(b, LAMP, 1, Normal);
        assert!(lv.has_scheduled_tick(a, LAMP) && lv.has_scheduled_tick(b, LAMP));
        assert!(!lv.will_tick_this_tick(a, LAMP) && !lv.will_tick_this_tick(b, LAMP));
        // a の実行中に、a 自身と b について 2 つの問い合わせを記録し、b をもう 1 件積む。
        lv.rules.on_tick.insert(
            a,
            vec![
                Act::Query(a, LAMP),
                Act::Query(b, LAMP),
                Act::Schedule(b, LAMP, 1, Normal),
                Act::Query(b, LAMP),
            ],
        );
        lv.tick();
        let queries: Vec<&Ev> = lv
            .rules
            .log
            .iter()
            .filter(|e| matches!(e, Ev::Query { .. }))
            .collect();
        assert_eq!(
            queries,
            [
                // 実行中の予約自身は、どちらにも無い。
                &Ev::Query {
                    pos: a,
                    has: false,
                    will: false
                },
                // b は取り出し済みで、まだ実行していない。
                &Ev::Query {
                    pos: b,
                    has: false,
                    will: true
                },
                // b をもう 1 件積めた。両方が真になる。
                &Ev::Query {
                    pos: b,
                    has: true,
                    will: true
                },
            ]
        );
        assert_eq!(ticked(&lv), [0, 1]);
        // 相が終われば、未実行分は空。積み直した b は次の tick に走る。
        assert!(!lv.will_tick_this_tick(b, LAMP));
        lv.tick();
        assert_eq!(ticked(&lv), [0, 1, 1]);
    }

    // 配送の時に種類を照合する。別のブロックに変わっていれば空振りし、
    // 同じ種類ならプロパティが変わっていても走る。予約は消費済みで、再予約もされない (V02 §4.6)。
    #[test]
    fn delivery_checks_block_kind_only() {
        let mut lv = level();
        let a = lamp_at(&mut lv, 0);
        let b = lamp_at(&mut lv, 1);
        let c = Pos::new(2, 0, 0);
        lv.schedule_tick(a, LAMP, 1, Normal);
        lv.schedule_tick(b, LAMP, 1, Normal);
        lv.schedule_tick(c, LAMP, 1, Normal); // 予約の時は空気。発火までにランプが置かれる
        lv.set_state_raw(a, block::STONE.default_state());
        lv.set_state_raw(b, crate::testing::lit_lamp());
        lv.set_state_raw(c, LAMP.default_state());
        lv.tick();
        assert_eq!(
            lv.rules.log,
            [
                Ev::Tick {
                    pos: b,
                    state: crate::testing::lit_lamp(),
                    time: 1
                },
                Ev::Tick {
                    pos: c,
                    state: LAMP.default_state(),
                    time: 1
                },
            ]
        );
        assert_eq!(lv.scheduled_tick_count(), 0);
        assert_eq!(lv.get_state(a), block::STONE.default_state());
    }

    // 上限に達したら、残りは入れ物に置いたままにし、TICK_CARRYOVER を返す (V02 §4.3)。
    // 残った分は集合にも残るので、同じ (位置, 種類) の新しい予約は捨てられる。次の tick に期限切れとして走る。
    #[test]
    fn carries_over_past_the_limit() {
        let mut lv = level_with(Config {
            max_ticks_per_tick: 3,
            ..Config::default()
        });
        let cells: Vec<Pos> = (0..5).map(|x| lamp_at(&mut lv, x)).collect();
        for pos in &cells {
            lv.schedule_tick(*pos, LAMP, 1, Normal);
        }
        assert_eq!(lv.tick(), Status::TICK_CARRYOVER);
        assert_eq!(ticked(&lv), [0, 1, 2]);
        assert!(lv.has_scheduled_tick(cells[3], LAMP));
        assert!(!lv.schedule_tick(cells[3], LAMP, 1, Normal));
        assert_eq!(lv.tick(), Status::OK);
        assert_eq!(ticked(&lv), [0, 1, 2, 3, 4]);
    }

    // 上限は、チャンクをまたいだ併合の途中でも効く。
    #[test]
    fn limit_applies_across_chunks() {
        let mut lv = level_with(Config {
            max_ticks_per_tick: 2,
            ..Config::default()
        });
        for x in [0, 16, 1] {
            let pos = lamp_at(&mut lv, x);
            lv.schedule_tick(pos, LAMP, 1, Normal);
        }
        assert_eq!(lv.tick(), Status::TICK_CARRYOVER);
        assert_eq!(ticked(&lv), [0, 16]);
        assert_eq!(lv.tick(), Status::OK);
        assert_eq!(ticked(&lv), [0, 16, 1]);
    }

    // 別のチャンクの先頭どうしが同点なら、黙って進めず LOADED_ORDER_TIE を立てる (V02 §5.5)。
    // 起動後に積んだ予約どうしは予約順が一意なので、同点は読み込んだ予約でしか起きない。
    #[test]
    fn ties_across_chunks_are_reported() {
        let mut ticks: LevelTicks<BlockId> = LevelTicks::new();
        for x in [0, 16] {
            ticks.schedule(ScheduledTick {
                kind: LAMP,
                pos: Pos::new(x, 0, 0),
                trigger: 1,
                priority: Normal,
                sub: -1,
            });
        }
        assert_eq!(ticks.collect(1, 65_536), Status::LOADED_ORDER_TIE);
        assert_eq!(ticks.run_now.len(), 2);
    }

    // 同点でない時は立たない。
    #[test]
    fn no_tie_without_equal_keys() {
        let mut ticks: LevelTicks<BlockId> = LevelTicks::new();
        for (x, sub) in [(0, 0), (16, 1)] {
            ticks.schedule(ScheduledTick {
                kind: LAMP,
                pos: Pos::new(x, 0, 0),
                trigger: 1,
                priority: Normal,
                sub,
            });
        }
        assert_eq!(ticks.collect(1, 65_536), Status::OK);
    }
}
