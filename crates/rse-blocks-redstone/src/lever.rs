//! レバー (V09 §4.1)。
//!
//! - 信号: 入っていれば、弱い信号 15 を全方向へ、強い信号 15 を取り付け先 (支え) にだけ出す。
//! - 右クリックで必ず切り替わる。予約 tick は使わない。
//! - 取り付け先の面が支えでなくなると壊れる。
//! - 置く時は、入力が運んだ face と facing をそのまま使う。置けるのは、取り付け先の面が支える時だけ。
//!
//! 音と GameEvent は持たない (乱数を使わないので、順にも効かない。V09 §4.1)。

use rse_core::state::block;
use rse_core::{Dir, Level, Placement, Pos, Rules, StateId, SupportType, UpdateFlags};

// プロパティの添字 (名前順: face・facing・powered。V05 §3.3)。
const FACE: usize = 0;
const FACING: usize = 1;
const POWERED: usize = 2;
// face の値の添字 (floor・wall・ceiling)。
const FACE_FLOOR: u8 = 0;
const FACE_CEILING: u8 = 2;
// 真偽の値の添字 (true・false)。
const TRUE: u8 = 0;

/// 入っているか。
#[inline]
pub fn is_powered(state: StateId) -> bool {
    state.value(POWERED) == TRUE
}

/// 取り付けの向き (V09 §3.1。FaceAttachedHorizontalDirectionalBlock.getConnectedDirection)。
/// 床付けは上、天井付けは下、壁付けは facing。支えは、この逆向きの隣にある。
pub fn connected_direction(state: StateId) -> Dir {
    match state.value(FACE) {
        FACE_FLOOR => Dir::Up,
        FACE_CEILING => Dir::Down,
        // 水平の向きの値の並びは 北・南・西・東 (V05 §3.2)。
        _ => match state.value(FACING) {
            0 => Dir::North,
            1 => Dir::South,
            2 => Dir::West,
            _ => Dir::East,
        },
    }
}

/// 支えの位置。
#[inline]
fn support_pos(pos: Pos, state: StateId) -> Pos {
    pos.offset(connected_direction(state).opposite())
}

/// 取り付いていられるか (V09 §4.1 の「取り付け」・V05 §4.7.5。
/// FaceAttachedHorizontalDirectionalBlock.canSurvive)。支えの、レバーに向いた面が FULL で支えること。
pub fn can_survive<R: Rules>(lv: &Level<R>, pos: Pos, state: StateId) -> bool {
    let toward_lever = connected_direction(state);
    lv.get_state(support_pos(pos, state))
        .is_face_sturdy(toward_lever, SupportType::Full)
}

/// 設置の時の状態 (V09 §3.1。FaceAttachedHorizontalDirectionalBlock.getStateForPlacement に当たる)。
/// `requested` は入力が運んだレバーの状態。face と facing は、そのまま使う。置かれるレバーは切れている。
///
/// - 取り付いていられるかは見ない。置く手順が、返した状態を [`can_survive`] に掛ける。
/// - 床置き・天井付けの facing は「置いた人の向き」(V09 §3.1) で、挙動には効かない。取り付けの向きは face だけで決まり
///   ([`connected_direction`])、facing を読むのは実験の世界の Orientation だけである (V09 §4.1)。
/// - UNVERIFIED: 押した面と見ている向きから face と facing を決める規則は、メモに無い。だから決めた後の値を入力で受ける
///   (redstone-sim#446 の質問 2)。実機は取り付けられる向きを探すので、要求の向きに支えが無い時に
///   別の向きで置かれることがありうる。ここでは置かない。
/// - UNVERIFIED: 入った状態 (powered が真) で置く組はメモに無いので、書かずに「規則が無い」を返す (同じ質問 2)。
pub fn placement_state(requested: StateId) -> Placement {
    if is_powered(requested) {
        Placement::Unimplemented
    } else {
        Placement::State(requested)
    }
}

/// 弱い信号 (V09 §4.1。LeverBlock.getSignal)。入っていれば、向きによらず 15。
pub fn signal(state: StateId) -> i32 {
    if is_powered(state) { 15 } else { 0 }
}

/// 強い信号 (V09 §4.1。LeverBlock.getDirectSignal)。
/// 入っていて、読み手が支えの時だけ 15。`dir` は読み手から源へ向かう向きなので、取り付けの向きと比べる。
pub fn direct_signal(state: StateId, dir: Dir) -> i32 {
    if is_powered(state) && dir == connected_direction(state) {
        15
    } else {
        0
    }
}

/// 自分の周り 6 マス → 支えの周り 6 マスの順に NC を出す (V09 §4.1。LeverBlock.updateNeighbours)。
/// 送り元はレバー。Orientation は実験の世界だけの物で、ここでは付けない。
fn update_neighbours<R: Rules>(lv: &mut Level<R>, pos: Pos, state: StateId) {
    lv.update_neighbors_at(pos, block::LEVER, None);
    lv.update_neighbors_at(support_pos(pos, state), block::LEVER, None);
}

/// 右クリック (V09 §4.1。LeverBlock.useWithoutItem → LeverBlock.pull)。条件は無く、必ず切り替わる。
///
/// 1. powered を反転した状態をフラグ 3 で書く (自分の周り 6 マスへの NC → SU)。
/// 2. 自分の周り 6 マス → 支えの周り 6 マスへ NC。
pub fn use_block<R: Rules>(lv: &mut Level<R>, pos: Pos, state: StateId) -> bool {
    let next = state.cycle(POWERED);
    lv.set_block(pos, next, UpdateFlags::ALL);
    update_neighbours(lv, pos, next);
    true
}

/// 撤去フック (V09 §4.1。LeverBlock.affectNeighborsAfterRemoval)。
/// ピストンの移動でなく、入っていた時だけ、周りへ NC を出す。この時点で自分の位置はもう別のブロックである。
/// 書き込みそのものの NC はこの後に出るので、自分の周り 6 マスには 2 回届く。
pub fn on_remove<R: Rules>(lv: &mut Level<R>, pos: Pos, old: StateId, moved: bool) {
    if !moved && is_powered(old) {
        update_neighbours(lv, pos, old);
    }
}

/// 形状更新 (V09 §4.1。FaceAttachedHorizontalDirectionalBlock.updateShape)。
/// 支えの向きから届いた時だけ、取り付いていられるかを確かめる。だめなら空気を返す (骨格が壊す)。
pub fn update_shape<R: Rules>(lv: &Level<R>, pos: Pos, state: StateId, dir: Dir) -> StateId {
    if dir == connected_direction(state).opposite() && !can_survive(lv, pos, state) {
        StateId::AIR
    } else {
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, level, put, stone};
    use rse_core::{Input, InputSection};

    const P: Pos = Pos::new(0, 1, 0);

    fn lever(text: &str) -> StateId {
        StateId::parse(text).unwrap()
    }

    // プロパティの添字が状態表と合っている。
    #[test]
    fn property_slots_match_the_table() {
        assert_eq!(block::LEVER.prop("face"), Some(FACE));
        assert_eq!(block::LEVER.prop("facing"), Some(FACING));
        assert_eq!(block::LEVER.prop("powered"), Some(POWERED));
        let s = lever("lever[face=ceiling,facing=east,powered=true]");
        assert_eq!(s.value(FACE), FACE_CEILING);
        assert!(is_powered(s));
        assert_eq!(lever("lever[face=floor]").value(FACE), FACE_FLOOR);
    }

    // 取り付けの向き: 床 → 上、天井 → 下、壁 → facing (V09 §3.1)。
    #[test]
    fn connected_direction_by_face() {
        assert_eq!(
            connected_direction(lever("lever[face=floor,facing=east]")),
            Dir::Up
        );
        assert_eq!(
            connected_direction(lever("lever[face=ceiling,facing=west]")),
            Dir::Down
        );
        for (name, dir) in [
            ("north", Dir::North),
            ("south", Dir::South),
            ("west", Dir::West),
            ("east", Dir::East),
        ] {
            let s = lever(&format!("lever[face=wall,facing={name}]"));
            assert_eq!(connected_direction(s), dir);
        }
    }

    // 右クリックで必ず切り替わる (V09 §4.1)。
    #[test]
    fn use_toggles_every_time() {
        let mut lv = level();
        put(&mut lv, P.offset(Dir::Down), stone());
        put(
            &mut lv,
            P,
            lever("lever[face=floor,facing=north,powered=false]"),
        );
        let use_it = Input::Use { pos: P };
        assert!(lv.input(InputSection::A, use_it).accepted);
        assert_eq!(at(&lv, P), "lever[face=floor,facing=north,powered=true]");
        assert!(lv.input(InputSection::A, use_it).accepted);
        assert_eq!(at(&lv, P), "lever[face=floor,facing=north,powered=false]");
    }

    // 弱い信号は向きによらず 15。強い信号は支えにだけ 15 (V09 §4.1)。
    #[test]
    fn signal_is_weak_everywhere_and_strong_to_support_only() {
        for text in [
            "lever[face=floor,facing=north,powered=true]",
            "lever[face=ceiling,facing=north,powered=true]",
            "lever[face=wall,facing=south,powered=true]",
            "lever[face=wall,facing=west,powered=true]",
        ] {
            let on = lever(text);
            let toward_lever = connected_direction(on);
            for dir in Dir::ALL {
                assert_eq!(signal(on), 15);
                let expect = if dir == toward_lever { 15 } else { 0 };
                assert_eq!(direct_signal(on, dir), expect, "{text} {dir:?}");
            }
            let off = on.cycle(POWERED);
            for dir in Dir::ALL {
                assert_eq!(signal(off), 0);
                assert_eq!(direct_signal(off, dir), 0);
            }
        }
    }

    // 入ったレバーは支えの石を強充電し、石は周りの読み手に 15 を見せる (V05 §2.3)。
    #[test]
    fn powered_lever_charges_its_support() {
        let mut lv = level();
        let support = P.offset(Dir::Down);
        put(&mut lv, support, stone());
        put(
            &mut lv,
            P,
            lever("lever[face=floor,facing=north,powered=true]"),
        );
        assert_eq!(lv.direct_signal_to(support), 15);
        assert!(lv.has_neighbor_signal(support.offset(Dir::East)));
        // 壁付けは、横の支えを強充電する。真下の石は強充電しない。
        let mut lv = level();
        let wall = P.offset(Dir::South);
        put(&mut lv, wall, stone());
        put(&mut lv, P.offset(Dir::Down), stone());
        put(
            &mut lv,
            P,
            lever("lever[face=wall,facing=north,powered=true]"),
        );
        assert_eq!(lv.direct_signal_to(wall), 15);
        assert_eq!(lv.direct_signal_to(P.offset(Dir::Down)), 0);
    }

    // 取り付いていられるのは、支えの面が FULL の時だけ (V05 §4.7.5)。レバーやダストには付かない。
    #[test]
    fn survives_only_on_a_sturdy_face() {
        let floor = lever("lever[face=floor,facing=north,powered=false]");
        let mut lv = level();
        assert!(!can_survive(&lv, P, floor));
        put(&mut lv, P.offset(Dir::Down), stone());
        assert!(can_survive(&lv, P, floor));
        put(&mut lv, P.offset(Dir::Down), floor);
        assert!(!can_survive(&lv, P, floor));
        // 壁付け (facing=north) の支えは南の隣。
        let wall = lever("lever[face=wall,facing=north,powered=false]");
        put(&mut lv, P.offset(Dir::North), stone());
        assert!(!can_survive(&lv, P, wall));
        put(&mut lv, P.offset(Dir::South), stone());
        assert!(can_survive(&lv, P, wall));
    }

    // 支えが消えると、支えの向きからの SU で壊れる (V09 §4.1。[実機 fixture lever-support-break])。
    // 支えでない隣が変わっても壊れない。
    #[test]
    fn breaks_when_support_goes_away() {
        let mut lv = level();
        let support = P.offset(Dir::Down);
        put(&mut lv, support, stone());
        put(&mut lv, P.offset(Dir::East), stone());
        put(
            &mut lv,
            P,
            lever("lever[face=floor,facing=north,powered=false]"),
        );
        lv.remove_block(P.offset(Dir::East), false);
        assert_eq!(at(&lv, P), "lever[face=floor,facing=north,powered=false]");
        lv.remove_block(support, false);
        assert_eq!(at(&lv, P), "air");
    }

    // 入ったレバーを壊すと、撤去フックが周りへ NC を出す。隣のランプは予約 tick で消える (V09 §4.1)。
    #[test]
    fn removal_of_a_powered_lever_updates_neighbors() {
        let mut lv = level();
        put(&mut lv, P.offset(Dir::Down), stone());
        put(
            &mut lv,
            P,
            lever("lever[face=floor,facing=north,powered=true]"),
        );
        let lamp = P.offset(Dir::East);
        put(
            &mut lv,
            lamp,
            StateId::parse("redstone_lamp[lit=true]").unwrap(),
        );
        lv.remove_block(P, false);
        assert_eq!(lv.scheduled_tick_count(), 1);
        for _ in 0..4 {
            lv.tick();
        }
        assert_eq!(at(&lv, lamp), "redstone_lamp[lit=false]");
    }

    // 設置の時の状態: face と facing は要求のまま。入った状態の要求は「規則が無い」(UNVERIFIED)。
    #[test]
    fn placement_keeps_face_and_facing() {
        for face in ["floor", "wall", "ceiling"] {
            for facing in ["north", "south", "west", "east"] {
                let off = lever(&format!("lever[face={face},facing={facing},powered=false]"));
                assert_eq!(placement_state(off), Placement::State(off));
                assert_eq!(
                    placement_state(off.cycle(POWERED)),
                    Placement::Unimplemented
                );
            }
        }
    }

    // 置く入力: 支えの面があれば置け、無ければ置けない (V09 §4.1)。置いただけでは周りへ信号を出さない。
    #[test]
    fn place_input_needs_a_sturdy_face() {
        let floor = lever("lever[face=floor,facing=east,powered=false]");
        let place = Input::Place {
            pos: P,
            state: floor,
        };
        let mut lv = level();
        assert!(!lv.input(InputSection::A, place).accepted);
        assert_eq!(at(&lv, P), "air");
        put(&mut lv, P.offset(Dir::Down), stone());
        let out = lv.input(InputSection::A, place);
        assert!(out.accepted && out.status.is_ok());
        assert_eq!(at(&lv, P), "lever[face=floor,facing=east,powered=false]");
        assert_eq!(lv.signal(P, Dir::West), 0);
        // 壁付け (facing=north) は南の隣が支え。下の石には付かない。
        let wall = lever("lever[face=wall,facing=north,powered=false]");
        let spot = P.offset(Dir::East);
        put(&mut lv, spot.offset(Dir::Down), stone());
        let place = Input::Place {
            pos: spot,
            state: wall,
        };
        assert!(!lv.input(InputSection::A, place).accepted);
        put(&mut lv, spot.offset(Dir::South), stone());
        assert!(lv.input(InputSection::A, place).accepted);
        assert_eq!(at(&lv, spot), "lever[face=wall,facing=north,powered=false]");
        // 入った状態の要求は書かずに、UNIMPLEMENTED を返す。
        let on = Input::Place {
            pos: P.offset(Dir::West),
            state: floor.cycle(POWERED),
        };
        put(&mut lv, P.offset(Dir::West).offset(Dir::Down), stone());
        let out = lv.input(InputSection::A, on);
        assert!(!out.accepted);
        assert_eq!(out.status, rse_core::Status::UNIMPLEMENTED);
        assert_eq!(at(&lv, P.offset(Dir::West)), "air");
    }
}
