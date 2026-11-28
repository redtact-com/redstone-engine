//! テスト用の小さな Rules。
//!
//! 素子は作らない。フックが呼ばれた順を記録し、位置ごとに仕込んだ動作を 1 回だけ実行する。
//! これで、骨格が決める順 (更新器・予約 tick・ブロックイベント・相) を素子なしで固定できる。
//!
//! 信号は、問い合わせを確かめるための最小の物だけを返す:
//! - 入ったレバー: 弱い信号 15 を全方向、強い信号 15 を真下の読み手にだけ
//! - ダスト: 弱い信号 = power を全方向

use crate::level::{Config, Level};
use crate::rules::{Placement, Rules};
use crate::set_block::UpdateFlags;
use crate::state::{Behavior, BlockId, StateId, block};
use crate::ticks::TickPriority;
use crate::update::Orientation;
use rse_java::{Dir, Pos};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// 記録する事象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    Nc {
        pos: Pos,
        src: BlockId,
    },
    /// 状態を持たせた NC (積んだ時点の状態が渡る)。
    NcState {
        pos: Pos,
        state: StateId,
    },
    Su {
        pos: Pos,
        dir: Dir,
        other: StateId,
    },
    Indirect {
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    },
    Place {
        pos: Pos,
        state: StateId,
        old: StateId,
        moved: bool,
    },
    Remove {
        pos: Pos,
        old: StateId,
        moved: bool,
    },
    Tick {
        pos: Pos,
        state: StateId,
        time: i64,
    },
    Event {
        pos: Pos,
        state: StateId,
        a: i32,
        b: i32,
    },
    Query {
        pos: Pos,
        has: bool,
        will: bool,
    },
    Snapshot {
        time: i64,
        handling_tick: bool,
    },
    Mark(&'static str),
}

/// 置く手順が呼んだ、読むだけの口の記録 ([`Probe::asked`])。
/// `hooks_before` は、呼ばれた時点で [`Probe::log`] に載っていた事象の数 (フックと更新より前かを見る)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asked {
    Placement {
        pos: Pos,
        requested: StateId,
        hooks_before: usize,
    },
    Survive {
        pos: Pos,
        state: StateId,
        hooks_before: usize,
    },
}

/// 仕込む動作。
#[derive(Clone, Copy, Debug)]
pub enum Act {
    Set(Pos, StateId, UpdateFlags),
    Schedule(Pos, BlockId, i32, TickPriority),
    Event(Pos, BlockId, i32, i32),
    UpdateNeighbors(Pos, BlockId),
    /// 状態を持たせた NC を積む。
    NcWithState(StateId, Pos),
    /// 予約 tick の 2 つの問い合わせを記録する。
    Query(Pos, BlockId),
    /// ゲーム時刻と handling_tick を記録する。
    Snapshot,
    Mark(&'static str),
}

type Script = BTreeMap<Pos3, Vec<Act>>;

/// 位置の鍵。Pos は Ord を持たないので、包んで順を付ける。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pos3(i32, i32, i32);

impl From<Pos> for Pos3 {
    fn from(p: Pos) -> Pos3 {
        Pos3(p.x, p.y, p.z)
    }
}

/// 仕込みの表。位置ごとに 1 回だけ実行する。
#[derive(Default)]
pub struct Scripts(Script);

impl Scripts {
    pub fn insert(&mut self, pos: Pos, acts: Vec<Act>) {
        self.0.insert(pos.into(), acts);
    }

    fn take(&mut self, pos: Pos) -> Option<Vec<Act>> {
        self.0.remove(&pos.into())
    }
}

/// テスト用の Rules。
#[derive(Default)]
pub struct Probe {
    pub log: Vec<Ev>,
    pub on_nc: Scripts,
    pub on_place: Scripts,
    pub on_remove: Scripts,
    pub on_tick: Scripts,
    pub on_event: Scripts,
    pub on_use: Scripts,
    /// 形状更新で返す状態 (位置ごとに 1 回)。
    pub shape_result: ShapeResults,
    /// 置いた時の状態の答え (位置ごと。使っても消えない)。仕込みが無ければ、要求の状態をそのまま置く。
    pub placement: Answers<Placement>,
    /// 支えの答え (位置ごと。使っても消えない)。仕込みが無ければ真。
    pub survive: Answers<bool>,
    /// 置いた時の状態と支えの口が呼ばれた記録。口は世界を書けない (`&Level`) ので、内側で書き換える。
    pub asked: RefCell<Vec<Asked>>,
}

/// 読むだけの口の答えの表。
pub struct Answers<T>(BTreeMap<Pos3, T>);

impl<T> Default for Answers<T> {
    fn default() -> Answers<T> {
        Answers(BTreeMap::new())
    }
}

impl<T: Copy> Answers<T> {
    pub fn insert(&mut self, pos: Pos, answer: T) {
        self.0.insert(pos.into(), answer);
    }

    fn get(&self, pos: Pos) -> Option<T> {
        self.0.get(&pos.into()).copied()
    }
}

#[derive(Default)]
pub struct ShapeResults(BTreeMap<Pos3, StateId>);

impl ShapeResults {
    pub fn insert(&mut self, pos: Pos, state: StateId) {
        self.0.insert(pos.into(), state);
    }
}

fn run(lv: &mut Level<Probe>, acts: Option<Vec<Act>>) -> bool {
    let Some(acts) = acts else {
        return false;
    };
    for act in acts {
        match act {
            Act::Set(pos, state, flags) => {
                lv.set_block(pos, state, flags);
            }
            Act::Schedule(pos, kind, delay, priority) => {
                lv.schedule_tick(pos, kind, delay, priority);
            }
            Act::Event(pos, kind, a, b) => {
                lv.block_event(pos, kind, a, b);
            }
            Act::UpdateNeighbors(pos, src) => lv.update_neighbors_at(pos, src, None),
            Act::NcWithState(state, pos) => {
                lv.neighbor_changed_with_state(state, pos, block::STONE, None, false)
            }
            Act::Query(pos, kind) => {
                let ev = Ev::Query {
                    pos,
                    has: lv.has_scheduled_tick(pos, kind),
                    will: lv.will_tick_this_tick(pos, kind),
                };
                lv.rules.log.push(ev);
            }
            Act::Snapshot => {
                let ev = Ev::Snapshot {
                    time: lv.game_time(),
                    handling_tick: lv.handling_tick(),
                };
                lv.rules.log.push(ev);
            }
            Act::Mark(text) => lv.rules.log.push(Ev::Mark(text)),
        }
    }
    true
}

impl Rules for Probe {
    fn neighbor_changed(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        src: BlockId,
        _ori: Option<Orientation>,
        moved: bool,
    ) {
        // 状態を持たせた NC は、渡った状態が今の状態と違うことがある。見分けて記録する。
        if moved || state != lv.get_state(pos) {
            lv.rules.log.push(Ev::NcState { pos, state });
        } else {
            lv.rules.log.push(Ev::Nc { pos, src });
        }
        let acts = lv.rules.on_nc.take(pos);
        run(lv, acts);
    }

    fn update_shape(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        dir: Dir,
        _other_pos: Pos,
        other: StateId,
    ) -> StateId {
        lv.rules.log.push(Ev::Su { pos, dir, other });
        lv.rules.shape_result.0.remove(&pos.into()).unwrap_or(state)
    }

    fn update_indirect_shapes(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    ) {
        lv.rules.log.push(Ev::Indirect {
            pos,
            state,
            flags,
            left,
        });
    }

    fn on_place(lv: &mut Level<Self>, pos: Pos, state: StateId, old: StateId, moved: bool) {
        lv.rules.log.push(Ev::Place {
            pos,
            state,
            old,
            moved,
        });
        let acts = lv.rules.on_place.take(pos);
        run(lv, acts);
    }

    fn on_remove(lv: &mut Level<Self>, pos: Pos, old: StateId, moved: bool) {
        lv.rules.log.push(Ev::Remove { pos, old, moved });
        let acts = lv.rules.on_remove.take(pos);
        run(lv, acts);
    }

    fn scheduled_tick(lv: &mut Level<Self>, pos: Pos, state: StateId) {
        let time = lv.game_time();
        lv.rules.log.push(Ev::Tick { pos, state, time });
        let acts = lv.rules.on_tick.take(pos);
        run(lv, acts);
    }

    fn block_event(lv: &mut Level<Self>, pos: Pos, state: StateId, a: i32, b: i32) -> bool {
        lv.rules.log.push(Ev::Event { pos, state, a, b });
        let acts = lv.rules.on_event.take(pos);
        run(lv, acts);
        true
    }

    fn use_block(lv: &mut Level<Self>, pos: Pos, _state: StateId) -> bool {
        let acts = lv.rules.on_use.take(pos);
        run(lv, acts)
    }

    fn placement_state(lv: &Level<Self>, pos: Pos, requested: StateId) -> Placement {
        lv.rules.asked.borrow_mut().push(Asked::Placement {
            pos,
            requested,
            hooks_before: lv.rules.log.len(),
        });
        lv.rules
            .placement
            .get(pos)
            .unwrap_or(Placement::State(requested))
    }

    fn can_survive(lv: &Level<Self>, pos: Pos, state: StateId) -> bool {
        lv.rules.asked.borrow_mut().push(Asked::Survive {
            pos,
            state,
            hooks_before: lv.rules.log.len(),
        });
        lv.rules.survive.get(pos).unwrap_or(true)
    }

    fn signal(_lv: &Level<Self>, _pos: Pos, state: StateId, _dir: Dir) -> i32 {
        match state.behavior() {
            Behavior::Lever if state == powered_lever() => 15,
            Behavior::Wire => state.value(WIRE_POWER) as i32,
            _ => 0,
        }
    }

    fn direct_signal(_lv: &Level<Self>, _pos: Pos, state: StateId, dir: Dir) -> i32 {
        // 読み手から源への向きが上 = 読み手は源の真下。
        if state == powered_lever() && dir == Dir::Up {
            15
        } else {
            0
        }
    }
}

/// ダストの power の、名前順の添字 (east, north, power, south, west)。
const WIRE_POWER: usize = 2;

pub fn level() -> Level<Probe> {
    level_with(Config::default())
}

pub fn level_with(config: Config) -> Level<Probe> {
    Level::new(Probe::default(), config)
}

pub fn lit_lamp() -> StateId {
    StateId::parse("redstone_lamp[lit=true]").unwrap()
}

pub fn powered_lever() -> StateId {
    StateId::parse("lever[face=floor,facing=north,powered=true]").unwrap()
}

pub fn wire_with_power(power: u8) -> StateId {
    block::REDSTONE_WIRE
        .default_state()
        .with_value(WIRE_POWER, power)
}
