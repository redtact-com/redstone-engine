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
#![no_std]
#![forbid(unsafe_code)]

pub use rse_core;
pub use rse_core::{
    Behavior, BlockId, Change, Config, Dir, Input, InputOutcome, InputSection, Pos, StateId,
    Status, UpdateFlags,
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
        // 渡された相手の状態は、今はどの素子にも渡していない。素子は世界から読む。
        // - ダストの水平と上: 自分で周りを読むので、渡された状態は判定に使わない (V06 §4.7 の表)。
        // - ダストの下 (支えの判定): UNVERIFIED。渡された状態で見るのか、世界の今の真下で見るのかは
        //   メモに無い (wire.rs の update_shape の下の腕)。メモが決まったら、ここで渡すかを直す。
        // - レバー: 支えの面を世界から読む (V09 §4.1)。
        let _ = (pos, dir, other_pos, other);
        match state.behavior() {
            Behavior::Inert => state,
            #[cfg(feature = "redstone")]
            Behavior::Wire => wire::update_shape(lv, pos, state, dir),
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
