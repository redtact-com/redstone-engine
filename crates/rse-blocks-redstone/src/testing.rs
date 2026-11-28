//! 族のテスト用の Rules。
//!
//! 族は rse-engine に依存できない (依存の向きが逆になる) ので、族だけで動かすための最小の振り分けをここに持つ。
//! 配る版の振り分けは rse-engine にある。ここはテストの時だけ組まれる。
//! 届いた NC (受け手と送り元) を記録する。

use crate::{RedstoneRules, RedstoneState, lamp, lever, wire};
use rse_core::state::{block, flag};
use rse_core::{
    Behavior, BlockId, Config, Dir, Level, Orientation, Placement, Pos, Rules, StateId, UpdateFlags,
};

#[derive(Default)]
pub struct Bench {
    redstone: RedstoneState,
    /// 届いた NC。(受け手, 送り元)。
    pub nc_log: Vec<(Pos, BlockId)>,
}

impl RedstoneRules for Bench {
    fn redstone(&self) -> &RedstoneState {
        &self.redstone
    }
    fn redstone_mut(&mut self) -> &mut RedstoneState {
        &mut self.redstone
    }
}

impl Rules for Bench {
    fn neighbor_changed(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        src: BlockId,
        _ori: Option<Orientation>,
        _moved: bool,
    ) {
        lv.rules.nc_log.push((pos, src));
        match state.behavior() {
            Behavior::Wire => wire::neighbor_changed(lv, pos, state),
            Behavior::Lamp => lamp::neighbor_changed(lv, pos, state),
            _ => {}
        }
    }

    fn update_shape(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        dir: Dir,
        _other_pos: Pos,
        other: StateId,
    ) -> StateId {
        match state.behavior() {
            Behavior::Wire => wire::update_shape(lv, pos, state, dir, other),
            Behavior::Lever => lever::update_shape(lv, pos, state, dir),
            _ => state,
        }
    }

    fn update_indirect_shapes(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    ) {
        if state.behavior() == Behavior::Wire {
            wire::update_indirect_shapes(lv, pos, state, flags, left);
        }
    }

    fn on_place(lv: &mut Level<Self>, pos: Pos, state: StateId, old: StateId, _moved: bool) {
        if state.behavior() == Behavior::Wire {
            wire::on_place(lv, pos, state, old);
        }
    }

    fn on_remove(lv: &mut Level<Self>, pos: Pos, old: StateId, moved: bool) {
        match old.behavior() {
            Behavior::Wire => wire::on_remove(lv, pos, old, moved),
            Behavior::Lever => lever::on_remove(lv, pos, old, moved),
            _ => {}
        }
    }

    fn scheduled_tick(lv: &mut Level<Self>, pos: Pos, state: StateId) {
        if state.behavior() == Behavior::Lamp {
            lamp::scheduled_tick(lv, pos, state);
        }
    }

    fn block_event(_lv: &mut Level<Self>, _pos: Pos, _state: StateId, _a: i32, _b: i32) -> bool {
        false
    }

    fn use_block(lv: &mut Level<Self>, pos: Pos, state: StateId) -> bool {
        match state.behavior() {
            Behavior::Lever => lever::use_block(lv, pos, state),
            Behavior::Wire => wire::use_block(lv, pos, state),
            _ => false,
        }
    }

    // 配る版の振り分け (rse-engine の Engine) と同じ形の match。族の 3 素子と、プロパティを持たない材料
    // (Inert のうち状態が 1 つの物) だけを置く。空気は置かない。ほかは規則が無い。
    fn placement_state(lv: &Level<Self>, pos: Pos, requested: StateId) -> Placement {
        match requested.behavior() {
            Behavior::Inert if requested.is_air() => Placement::Refused,
            Behavior::Inert if requested.block().state_count() == 1 => Placement::State(requested),
            Behavior::Wire => Placement::State(wire::placement_state(lv, pos)),
            Behavior::Lever => lever::placement_state(requested),
            Behavior::Lamp => Placement::State(lamp::placement_state(lv, pos)),
            _ => Placement::Unimplemented,
        }
    }

    // rse-engine と同じ: 材料とランプは支えの条件を持たない。規則の無い種類は置く手順から届かず、届いたら置かない側に倒す。
    fn can_survive(lv: &Level<Self>, pos: Pos, state: StateId) -> bool {
        match state.behavior() {
            Behavior::Inert | Behavior::Lamp => true,
            Behavior::Wire => wire::can_survive(lv, pos),
            Behavior::Lever => lever::can_survive(lv, pos, state),
            _ => false,
        }
    }

    fn signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32 {
        match state.behavior() {
            Behavior::Wire => wire::signal(lv, pos, state, dir),
            Behavior::Lever => lever::signal(state),
            _ => 0,
        }
    }

    fn direct_signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32 {
        match state.behavior() {
            Behavior::Wire => wire::direct_signal(lv, pos, state, dir),
            Behavior::Lever => lever::direct_signal(state, dir),
            _ => 0,
        }
    }

    fn is_signal_source(lv: &Level<Self>, state: StateId) -> bool {
        match state.behavior() {
            Behavior::Wire => lv.rules.redstone.wire_gives_signal(),
            _ => state.has(flag::SIGNAL_SOURCE),
        }
    }
}

pub fn level() -> Level<Bench> {
    Level::new(Bench::default(), Config::default())
}

pub fn stone() -> StateId {
    block::STONE.default_state()
}

/// 更新を出さずに置く。
pub fn put(lv: &mut Level<Bench>, pos: Pos, state: StateId) {
    assert!(lv.set_state_raw(pos, state));
}

/// 今の状態を blockstate の文字列で読む。
pub fn at(lv: &Level<Bench>, pos: Pos) -> String {
    lv.get_state(pos).to_string()
}
