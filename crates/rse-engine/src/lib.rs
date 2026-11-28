//! 組み立て。
//!
//! rse-core の `Rules` をここで 1 つだけ実装し、状態表の振る舞いの種類を match して
//! 族の関数を呼ぶ。外した族の素子は「未実装」の状態コードを返す (docs/architecture.md §3)。
//!
//! - 振り分けはこのファイルの match だけにある。族の crate は素子ごとの関数を持つだけで、互いを知らない。
//! - フックを呼ぶ条件と順は骨格が持つ。ここは「どの素子の関数へ渡すか」だけを決める。
//! - 族の腕には feature の条件を付ける。feature を外すと、その腕は末尾の「未実装」の腕に落ちる。
//!   まだ族の crate が無い種類 (リピーター・ピストンほか) も、同じ腕に落ちる。
//! - 信号の問い合わせは世界を書き換えられないので、未実装の種類は 0 を返すだけになる。
//!   状態コードは、その素子が NC・SU・設置・撤去・予約 tick・入力のどれかを受けた時に立つ。
//! - 置く入力の 2 つの口 (置いた時の状態・支え) も世界を書き換えられない。未実装の種類は、置いた時の状態の口が
//!   「規則が無い」を返し、骨格が状態コードを立てて何も書かない。
#![no_std]
#![forbid(unsafe_code)]
// doctest には crate の forbid が効かないので、doctest にも付ける (docs/wasm-abi.md §5)。今は doctest が無い。
#![doc(test(attr(forbid(unsafe_code))))]

pub use rse_core;
pub use rse_core::{
    Behavior, BlockId, Change, Config, Dir, Input, InputOutcome, InputSection, Placement, Pos,
    StateId, Status, UpdateFlags,
};

#[cfg(feature = "redstone")]
use rse_blocks_redstone::{RedstoneRules, RedstoneState, lamp, lever, wire};
use rse_core::{Level, Orientation, Rules};

/// 配る版の世界。
pub type World = Level<Engine>;

/// 新しい世界を作る。
pub fn new_world(config: Config) -> World {
    Level::new(Engine::default(), config)
}

/// 素子の振る舞いの振り分け。値は、族が世界ごとに持つ状態の入れ物である。
#[derive(Default)]
pub struct Engine {
    #[cfg(feature = "redstone")]
    redstone: RedstoneState,
}

#[cfg(feature = "redstone")]
impl RedstoneRules for Engine {
    fn redstone(&self) -> &RedstoneState {
        &self.redstone
    }
    fn redstone_mut(&mut self) -> &mut RedstoneState {
        &mut self.redstone
    }
}

impl Rules for Engine {
    fn neighbor_changed(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        src: BlockId,
        ori: Option<Orientation>,
        moved: bool,
    ) {
        // 縦切りの 3 素子は、送り元・Orientation・「ピストンで動いたか」を読まない (V06 §4.11・V11 §4.1)。
        let _ = (pos, src, ori, moved);
        match state.behavior() {
            Behavior::Inert => {}
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::neighbor_changed(lv, pos, state),
            // レバーは NC に反応しない。取り付けは SU で確かめる (V09 §4.1)。
            #[cfg(feature = "redstone")]
            Behavior::Lever => {}
            #[cfg(feature = "redstone")]
            Behavior::Lamp => lamp::neighbor_changed(lv, pos, state),
            _ => lv.raise(Status::UNIMPLEMENTED),
        }
    }

    fn update_shape(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        dir: Dir,
        other_pos: Pos,
        other: StateId,
    ) -> StateId {
        // 運ばれた相手の状態 (積んだ時点の物。V04 §5.5) を使うのは、ダストの下の腕だけ。
        // - ダストの下 (支えの判定): 運ばれた状態で判定し、世界の今の真下は読まない (V06 §4.6)。
        // - ダストの水平と上: 自分で周りを読むので、運ばれた状態は判定に使わない (V06 §4.6・§4.7)。
        // - レバー: 運ばれた状態は使わず、世界の今の支えを読む (V09 §4.1)。
        let _ = (pos, dir, other_pos, other);
        match state.behavior() {
            Behavior::Inert => state,
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::update_shape(lv, pos, state, dir, other),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::update_shape(lv, pos, state, dir),
            // ランプは形を持たない。受電は SU では決まらない (V11 §5.2)。
            #[cfg(feature = "redstone")]
            Behavior::Lamp => state,
            _ => {
                lv.raise(Status::UNIMPLEMENTED);
                state
            }
        }
    }

    fn update_indirect_shapes(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    ) {
        let _ = (pos, flags, left);
        match state.behavior() {
            Behavior::Inert => {}
            // 中身があるのはダストだけ (V06 §4.7)。
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::update_indirect_shapes(lv, pos, state, flags, left),
            #[cfg(feature = "redstone")]
            Behavior::Lever | Behavior::Lamp => {}
            _ => lv.raise(Status::UNIMPLEMENTED),
        }
    }

    fn on_place(lv: &mut Level<Self>, pos: Pos, state: StateId, old: StateId, moved: bool) {
        let _ = (pos, old, moved);
        match state.behavior() {
            Behavior::Inert => {}
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::on_place(lv, pos, state, old),
            // レバーとランプは設置フックを持たない (V09 §4.1・V11 §4.1)。
            #[cfg(feature = "redstone")]
            Behavior::Lever | Behavior::Lamp => {}
            _ => lv.raise(Status::UNIMPLEMENTED),
        }
    }

    fn on_remove(lv: &mut Level<Self>, pos: Pos, old: StateId, moved: bool) {
        let _ = (pos, moved);
        match old.behavior() {
            Behavior::Inert => {}
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::on_remove(lv, pos, old, moved),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::on_remove(lv, pos, old, moved),
            #[cfg(feature = "redstone")]
            Behavior::Lamp => {}
            _ => lv.raise(Status::UNIMPLEMENTED),
        }
    }

    fn scheduled_tick(lv: &mut Level<Self>, pos: Pos, state: StateId) {
        let _ = pos;
        match state.behavior() {
            Behavior::Inert => {}
            // ダストとレバーは予約 tick を使わない (V06 §3.3・V09 §4.1)。届いても何もしない。
            #[cfg(feature = "redstone")]
            Behavior::Wire | Behavior::Lever => {}
            #[cfg(feature = "redstone")]
            Behavior::Lamp => lamp::scheduled_tick(lv, pos, state),
            _ => lv.raise(Status::UNIMPLEMENTED),
        }
    }

    fn block_event(lv: &mut Level<Self>, pos: Pos, state: StateId, a: i32, b: i32) -> bool {
        let _ = (pos, a, b);
        match state.behavior() {
            Behavior::Inert => false,
            // 縦切りの 3 素子はブロックイベントを使わない。
            #[cfg(feature = "redstone")]
            Behavior::Wire | Behavior::Lever | Behavior::Lamp => false,
            _ => {
                lv.raise(Status::UNIMPLEMENTED);
                false
            }
        }
    }

    fn use_block(lv: &mut Level<Self>, pos: Pos, state: StateId) -> bool {
        let _ = pos;
        match state.behavior() {
            Behavior::Inert => false,
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::use_block(lv, pos, state),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::use_block(lv, pos, state),
            #[cfg(feature = "redstone")]
            Behavior::Lamp => false,
            _ => {
                lv.raise(Status::UNIMPLEMENTED);
                false
            }
        }
    }

    fn placement_state(lv: &Level<Self>, pos: Pos, requested: StateId) -> Placement {
        let _ = (lv, pos);
        match requested.behavior() {
            // 空気は置く物ではない。
            Behavior::Inert if requested.is_air() => Placement::Refused,
            // プロパティを持たない材料 (石) は状態が 1 つなので、それを置く。
            // プロパティを持つ材料は、置いた時の規則 (向きほか) がまだ無い。黙って要求の状態を置かない。
            Behavior::Inert if requested.block().state_count() == 1 => Placement::State(requested),
            // ダストは要求のプロパティを読まず、周りから形を決める。power は 0 (V06 §4.4)。
            #[cfg(feature = "redstone")]
            Behavior::Wire => Placement::State(wire::placement_state(lv, pos)),
            // レバーは face と facing を要求のまま使い、powered は要求によらず偽にする (V09 §4.1)。
            #[cfg(feature = "redstone")]
            Behavior::Lever => Placement::State(lever::placement_state(requested)),
            // ランプは、置く位置が受電していれば点いて置かれる (V11 §4.1)。
            #[cfg(feature = "redstone")]
            Behavior::Lamp => Placement::State(lamp::placement_state(lv, pos)),
            _ => Placement::Unimplemented,
        }
    }

    fn can_survive(lv: &Level<Self>, pos: Pos, state: StateId) -> bool {
        let _ = (lv, pos);
        match state.behavior() {
            // 材料 (石) は canSurvive を上書きせず、既定の「常に真」を使う (V11 §4.1)。
            Behavior::Inert => true,
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::can_survive(lv, pos),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::can_survive(lv, pos, state),
            // ランプも上書きしない。宙にも置ける (V11 §4.1)。
            #[cfg(feature = "redstone")]
            Behavior::Lamp => true,
            // 未実装の種類は、置いた時の状態の口が先に止めるので、置く手順からはここへ来ない。
            // 来た時は置かない側に倒す。
            _ => false,
        }
    }

    fn signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32 {
        let _ = (lv, pos, dir);
        match state.behavior() {
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::signal(lv, pos, state, dir),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::signal(state),
            // 材料・ランプは自分の信号を持たない。未実装の種類もここへ落ちる (冒頭の注)。
            _ => 0,
        }
    }

    fn direct_signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32 {
        let _ = (lv, pos, dir);
        match state.behavior() {
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::direct_signal(lv, pos, state, dir),
            #[cfg(feature = "redstone")]
            Behavior::Lever => lever::direct_signal(state, dir),
            _ => 0,
        }
    }

    fn is_signal_source(lv: &Level<Self>, state: StateId) -> bool {
        let _ = lv;
        match state.behavior() {
            // ダストは、ほかのダストが周りを読む間だけ信号源でなくなる (V05 §7.2・V06 §4.8)。
            #[cfg(feature = "redstone")]
            Behavior::Wire => lv.rules.redstone.wire_gives_signal(),
            _ => state.has(rse_core::state::flag::SIGNAL_SOURCE),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use rse_core::state::block;
    #[cfg(feature = "redstone")]
    use std::string::ToString;

    fn parse(text: &str) -> StateId {
        StateId::parse(text).unwrap()
    }

    /// 石の床の上に、レバー → ダスト → ランプを並べる (更新を出さずに置く)。
    fn lever_wire_lamp() -> (World, Pos, Pos, Pos) {
        let mut world = new_world(Config::default());
        let (lever, wire, lamp) = (Pos::new(0, 1, 0), Pos::new(1, 1, 0), Pos::new(2, 1, 0));
        for x in 0..=2 {
            world.set_state_raw(Pos::new(x, 0, 0), block::STONE.default_state());
        }
        world.set_state_raw(lever, parse("lever[face=floor,facing=north,powered=false]"));
        world.set_state_raw(
            wire,
            parse("redstone_wire[east=side,north=none,power=0,south=none,west=side]"),
        );
        world.set_state_raw(lamp, parse("redstone_lamp[lit=false]"));
        (world, lever, wire, lamp)
    }

    // 族を入れた組み立て: レバー → ダスト → ランプが、振り分けを通して動く。
    #[cfg(feature = "redstone")]
    #[test]
    fn redstone_family_is_dispatched() {
        let (mut world, lever, wire, lamp) = lever_wire_lamp();
        let outcome = world.input(InputSection::A, Input::Use { pos: lever });
        assert!(outcome.accepted);
        assert!(outcome.status.is_ok());
        assert_eq!(
            world.get_state(wire).to_string(),
            "redstone_wire[east=side,north=none,power=15,south=none,west=side]"
        );
        assert_eq!(world.get_state(lamp).to_string(), "redstone_lamp[lit=true]");
        // 戻すと、ダストはその場で 0 になり、ランプは 4gt 後に消える (V11 §4.1)。
        world.input(InputSection::A, Input::Use { pos: lever });
        assert_eq!(world.get_state(lamp).to_string(), "redstone_lamp[lit=true]");
        for _ in 0..4 {
            assert!(world.tick().is_ok());
        }
        assert_eq!(
            world.get_state(lamp).to_string(),
            "redstone_lamp[lit=false]"
        );
        // ダストは、評価の外では信号源 (V05 §7.2)。
        assert!(world.is_signal_source(world.get_state(wire)));
    }

    // 族を外した組み立て: 族の素子は黙って動かない形にならず、UNIMPLEMENTED を返す (docs/architecture.md §3)。
    #[cfg(not(feature = "redstone"))]
    #[test]
    fn family_blocks_are_unimplemented_without_the_feature() {
        let (mut world, lever, wire, lamp) = lever_wire_lamp();
        // 入力。
        for pos in [lever, wire, lamp] {
            let before = world.get_state(pos);
            let outcome = world.input(InputSection::A, Input::Use { pos });
            assert!(!outcome.accepted);
            assert_eq!(outcome.status, Status::UNIMPLEMENTED);
            assert_eq!(world.get_state(pos), before);
        }
        // NC。
        for pos in [lever, wire, lamp] {
            world.neighbor_changed(pos, block::STONE, None);
            assert_eq!(world.take_status(), Status::UNIMPLEMENTED);
        }
        // 設置と撤去 (フラグ 3 の書き込み)。
        let spot = Pos::new(5, 1, 0);
        for text in ["lever", "redstone_wire", "redstone_lamp"] {
            assert!(world.set_block(spot, parse(text), UpdateFlags::ALL));
            assert_eq!(world.take_status(), Status::UNIMPLEMENTED, "{text} の設置");
            assert!(world.remove_block(spot, false));
            assert_eq!(world.take_status(), Status::UNIMPLEMENTED, "{text} の撤去");
        }
        // 置く入力: 族の素子は書かずに UNIMPLEMENTED。壊す入力は、撤去フックで UNIMPLEMENTED が立つ (消えはする)。
        for text in ["lever[face=floor]", "redstone_wire", "redstone_lamp"] {
            let place = Input::Place {
                pos: spot,
                state: parse(text),
            };
            let outcome = world.input(InputSection::A, place);
            assert!(!outcome.accepted, "{text}");
            assert_eq!(outcome.status, Status::UNIMPLEMENTED, "{text}");
            assert_eq!(world.get_state(spot), StateId::AIR, "{text}");
        }
        let outcome = world.input(InputSection::A, Input::Break { pos: wire });
        assert!(outcome.accepted);
        assert_eq!(outcome.status, Status::UNIMPLEMENTED);
        world.set_state_raw(wire, parse("redstone_wire[east=side,west=side]"));
        // 予約 tick。
        world.schedule_tick(
            lamp,
            block::REDSTONE_LAMP,
            1,
            rse_core::TickPriority::Normal,
        );
        assert_eq!(world.tick(), Status::UNIMPLEMENTED);
        // 信号は 0。信号源の印は表の値のまま。
        assert_eq!(world.signal(lever, Dir::West), 0);
        assert!(world.is_signal_source(world.get_state(wire)));
    }

    // 振り分けは、運ばれた相手の状態をダストにだけ渡す。
    // ダストの下の腕は運ばれた状態で判定し (V06 §4.6)、レバーは世界の今の支えで判定する (V09 §4.1)。
    #[cfg(feature = "redstone")]
    #[test]
    fn carried_state_reaches_the_wire_but_not_the_lever() {
        let stone = block::STONE.default_state();
        let down = |world: &mut World, pos: Pos, carried: StateId| {
            let state = world.get_state(pos);
            Engine::update_shape(world, pos, state, Dir::Down, pos.offset(Dir::Down), carried)
        };

        // 世界の真下は石のまま。支えない状態 (空気) が運ばれた時。
        let (mut world, lever, wire, _) = lever_wire_lamp();
        let (lever_state, wire_state) = (world.get_state(lever), world.get_state(wire));
        assert_eq!(down(&mut world, wire, StateId::AIR), StateId::AIR);
        assert_eq!(down(&mut world, lever, StateId::AIR), lever_state);
        assert_eq!(down(&mut world, wire, stone), wire_state);

        // 世界の真下は空気。支える状態 (石) が運ばれた時。
        let (mut world, lever, wire, _) = lever_wire_lamp();
        world.set_state_raw(wire.offset(Dir::Down), StateId::AIR);
        world.set_state_raw(lever.offset(Dir::Down), StateId::AIR);
        assert_eq!(down(&mut world, wire, stone), wire_state);
        assert_eq!(down(&mut world, lever, stone), StateId::AIR);
        assert!(world.take_status().is_ok());
    }

    // 置く入力の材料の腕は、どちらの組み立てでも同じ: 石は置ける (支えは要らない)。空気は置かない。
    // 空気でない所には置けない。壊すと空気に戻る。状態コードは立たない。
    #[test]
    fn inert_blocks_place_and_break_in_any_build() {
        let mut world = new_world(Config::default());
        let stone = block::STONE.default_state();
        let spot = Pos::new(0, 5, 0);
        let place = |world: &mut World, state| {
            world.input(InputSection::A, Input::Place { pos: spot, state })
        };
        let refused = InputOutcome {
            accepted: false,
            status: Status::OK,
        };
        assert_eq!(place(&mut world, StateId::AIR), refused);
        assert!(place(&mut world, stone).accepted);
        assert_eq!(world.get_state(spot), stone);
        assert_eq!(place(&mut world, stone), refused);
        let broke = world.input(InputSection::A, Input::Break { pos: spot });
        assert!(broke.accepted && broke.status.is_ok());
        assert_eq!(world.get_state(spot), StateId::AIR);
        assert_eq!(
            world.input(InputSection::A, Input::Break { pos: spot }),
            refused
        );
        assert_eq!(world.scheduled_tick_count(), 0);
        assert_eq!(world.game_time(), 0);
    }

    // 材料 (石・空気) は、どちらの組み立てでも何も立てない。
    #[test]
    fn inert_blocks_never_raise() {
        let mut world = new_world(Config::default());
        let spot = Pos::new(0, 0, 0);
        assert!(world.set_block(spot, block::STONE.default_state(), UpdateFlags::ALL));
        assert!(
            !world
                .input(InputSection::A, Input::Use { pos: spot })
                .accepted
        );
        assert!(world.remove_block(spot, false));
        assert!(world.tick().is_ok());
        assert!(world.take_status().is_ok());
    }
}
