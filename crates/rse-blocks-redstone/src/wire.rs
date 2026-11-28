//! レッドストーンダスト (V06)。既定の評価器だけを持つ。
//!
//! 仕事は 3 つで、互いに別の契機で動く (V06 §2.1)。
//! - 形 (4 方向の接続) の維持: 形状更新で動く。電力は計算しない。書き込みは NC を出さない。
//! - 電力の計算: 近隣更新・設置・除去で動く。1 本ずつ計算して書き、7 起点 × 6 方向の NC を出す。
//!   途中の値をそのつど世界に書く。最終の値を先に求める近道は使わない (V06 §7.3)。
//! - 信号の出力: 問い合わせのたびに周りを見て接続を計算し直す。持っている形で効くのは「点か」だけ (V06 §5.2)。
//!
//! 実験の評価器 (V06 §4.13) と Orientation は持たない。既定の世界では使わない (V06 §4.17)。
//! アイテムのドロップも持たない。
//!
//! ダストが自分で配る NC の送り元は、どれもダストである (V06 §4.11 の「ダストが出す近隣更新の送り主」)。
//! 評価器の 7 起点・設置の上下・角の確認・除去の 6 起点・右クリックの導体の先のどれも、
//! 起点がダストでないマスでも、送り元は起点のブロックではなくダストにする。
//! 除去では世界のそのマスはもう別のブロックだが、送り元は消えたダストである。

use crate::RedstoneRules;
use rse_core::state::block;
use rse_core::{Behavior, Dir, JavaHashSet, Level, Pos, StateId, Status, SupportType, UpdateFlags};

// プロパティの添字 (名前順: east・north・power・south・west。V05 §3.3)。
const EAST: usize = 0;
const NORTH: usize = 1;
const POWER: usize = 2;
const SOUTH: usize = 3;
const WEST: usize = 4;

/// 水平の並び (北・東・南・西。V06 §3.6) の順に並べた、接続のプロパティの添字。
const SIDE_PROPS: [usize; 4] = [NORTH, EAST, SOUTH, WEST];
// [`Shape`] の中の添字。水平の並びの順。
const N: usize = 0;
const E: usize = 1;
const S: usize = 2;
const W: usize = 3;

/// 接続の値 (V06 §3.1。RedstoneSide)。番号は状態表の値の添字 (up・side・none)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Side {
    Up = 0,
    Side = 1,
    None = 2,
}

impl Side {
    /// 接続しているか。none 以外のこと (V06 §3.1)。
    #[inline]
    pub const fn is_connected(self) -> bool {
        !matches!(self, Side::None)
    }

    const fn from_value(value: u8) -> Side {
        match value {
            0 => Side::Up,
            1 => Side::Side,
            _ => Side::None,
        }
    }
}

/// 4 方向の接続。添字は水平の並び (北・東・南・西)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape(pub [Side; 4]);

impl Shape {
    /// 点: 4 方向とも none (V06 §3.2)。
    pub const DOT: Shape = Shape([Side::None; 4]);
    /// 十字の基準: 4 方向とも side (V06 §3.2)。
    pub const CROSS: Shape = Shape([Side::Side; 4]);

    /// 点か。
    pub fn is_dot(self) -> bool {
        self.0.iter().all(|s| !s.is_connected())
    }

    /// 十字か。4 方向とも接続 (up でも side でもよい。V06 §3.2)。
    pub fn is_cross(self) -> bool {
        self.0.iter().all(|s| s.is_connected())
    }

    /// 状態が持っている形を読む。
    pub fn of(state: StateId) -> Shape {
        Shape(SIDE_PROPS.map(|prop| Side::from_value(state.value(prop))))
    }

    /// 状態の形だけを差し替える。power は変えない。
    pub fn apply(self, mut state: StateId) -> StateId {
        for (prop, side) in SIDE_PROPS.into_iter().zip(self.0) {
            state = state.with_value(prop, side as u8);
        }
        state
    }
}

/// 水平の方向の、[`Shape`] の中の添字。垂直の方向には無い。
fn slot(dir: Dir) -> Option<usize> {
    Dir::HORIZONTAL.iter().position(|d| *d == dir)
}

/// 状態が持っている power。
#[inline]
pub fn power_of(state: StateId) -> u8 {
    state.value(POWER)
}

#[inline]
fn is_wire(state: StateId) -> bool {
    state.behavior() == Behavior::Wire
}

/// その状態が、上に載るダストの支えになるか (V06 §4.5・V05 §4.7.5。RedstoneWireBlock.canSurviveOn)。
/// 上面が FULL で支えるか、ホッパーなら支えになる。世界を読まない。
///
/// 動的形状のブロック (面の形を世界と合わせて決める物。V06 §4.6・V05 §4.7.4) は、まだ状態表に無い。
/// 入れる時は、真下の座標も受けて面の形を求める。
#[inline]
pub fn can_survive_on(below: StateId) -> bool {
    below.is_face_sturdy(Dir::Up, SupportType::Full) || below.behavior() == Behavior::Hopper
}

/// 支え (V06 §4.5。RedstoneWireBlock.canSurvive)。世界の今の真下で判定する。
/// 近隣更新の経路が使う (V06 §4.11 の手順 2・3)。下からの形状更新は使わない ([`update_shape`])。
pub fn can_survive<R: RedstoneRules>(lv: &Level<R>, pos: Pos) -> bool {
    can_survive_on(lv.get_state(pos.offset(Dir::Down)))
}

/// 接続先になるか (V06 §4.1。BlockBehaviour.shouldRedstoneWireConnectTo)。
/// `dir` はダストから相手へ向かう方向。斜めの相手を調べる時は「向き無し」で、真になるのはダストだけ。
///
/// リピーターとオブザーバーは向きで答えが変わる (V06 §4.1 の表)。どちらもまだ状態表に無く、ここへは届かない。
/// 族が入る PR で、向きごとの答えを状態表の性質の列にして引く (族どうしは依存しない)。それまでは偽を返す席である。
fn connects_to<R: RedstoneRules>(lv: &Level<R>, state: StateId, dir: Option<Dir>) -> bool {
    match state.behavior() {
        Behavior::Wire => true,
        Behavior::Repeater | Behavior::Observer => false,
        _ => dir.is_some() && lv.is_signal_source(state),
    }
}

/// 上へ接続できるか (V06 §4.2)。ダストの真上が導体でないこと。
/// 実機は真上の状態の導体の判定に p の座標を渡すが、座標で変わる導体は今の表に無い (V06 §4.2 の落とし穴)。
fn can_go_up<R: RedstoneRules>(lv: &Level<R>, pos: Pos) -> bool {
    !lv.get_state(pos.offset(Dir::Up)).is_conductor()
}

/// 1 方向の生の接続 (V06 §4.2。RedstoneWireBlock.getConnectingSide)。
///
/// 上りの判定が先。横のブロック n の上にダストを置けて、n の真上がダストなら、ここで決まる
/// (n の、ダスト側の側面が支えるなら up、支えなければ side)。
/// 決まらなければ、n が接続先か、n が導体でなく n の真下がダストなら side。どちらでもなければ none。
fn connecting_side<R: RedstoneRules>(lv: &Level<R>, pos: Pos, dir: Dir, go_up: bool) -> Side {
    let near = pos.offset(dir);
    let near_state = lv.get_state(near);
    if go_up {
        let can_hold_wire = near_state.behavior() == Behavior::Trapdoor
            || near_state.is_face_sturdy(Dir::Up, SupportType::Full)
            || near_state.behavior() == Behavior::Hopper;
        if can_hold_wire && connects_to(lv, lv.get_state(near.offset(Dir::Up)), None) {
            return if near_state.is_face_sturdy(dir.opposite(), SupportType::Full) {
                Side::Up
            } else {
                Side::Side
            };
        }
    }
    let goes_down =
        !near_state.is_conductor() && connects_to(lv, lv.get_state(near.offset(Dir::Down)), None);
    if connects_to(lv, near_state, Some(dir)) || goes_down {
        Side::Side
    } else {
        Side::None
    }
}

/// 4 方向の接続と自動の延長 (V06 §4.3。RedstoneWireBlock.getConnectionState)。
/// `held` は入力の形 (ふつうは状態が持っている形)。使うのは「点か」だけ。
///
/// 1. 北・東・南・西の順に、生の接続を求める。
/// 2. 入力が点で、生の接続も 0 本なら、点のまま (点の保持)。
/// 3. そうでなければ延長する。「南北が空」「東西が空」は延長の前に求め、途中で変えない。
///    西・東は南北が空なら side に、北・南は東西が空なら side にする。
///
/// 結果は、0 本 → 十字、1 本 → 直線、2 本以上 → そのまま。
/// [実機 fixture dust-cross-powering・dust-line-powering]
pub fn connection_state<R: RedstoneRules>(lv: &Level<R>, pos: Pos, held: Shape) -> Shape {
    let go_up = can_go_up(lv, pos);
    let mut out = Shape::DOT;
    for (i, dir) in Dir::HORIZONTAL.into_iter().enumerate() {
        out.0[i] = connecting_side(lv, pos, dir, go_up);
    }
    if held.is_dot() && out.is_dot() {
        return out;
    }
    let north_south_empty = !out.0[N].is_connected() && !out.0[S].is_connected();
    let east_west_empty = !out.0[E].is_connected() && !out.0[W].is_connected();
    for (i, empty) in [
        (W, north_south_empty),
        (E, north_south_empty),
        (N, east_west_empty),
        (S, east_west_empty),
    ] {
        if !out.0[i].is_connected() && empty {
            out.0[i] = Side::Side;
        }
    }
    out
}

/// 設置の時の状態 (V06 §4.4。RedstoneWireBlock.getStateForPlacement)。
/// 十字の基準を入力にするので、孤立していれば十字になる。power は 0。
pub fn placement_state<R: RedstoneRules>(lv: &Level<R>, pos: Pos) -> StateId {
    connection_state(lv, pos, Shape::CROSS).apply(block::REDSTONE_WIRE.default_state())
}

/// 形状更新 (V06 §4.6。RedstoneWireBlock.updateShape)。`dir` はダストから、変わった隣へ向かう方向。
/// `other` は形状更新が運んできた隣の状態で、積んだ時点の物である。今の世界の状態とは限らない (V04 §5.5)。
///
/// - 下: 運ばれた状態が支えにならなければ空気を返す (骨格が壊す)。世界の今の真下は読まない
///   (V06 §4.6 の「下の段で見る状態」)。近隣更新の経路は逆で、世界の今の真下を読む ([`can_survive`])。
///   2 つが食い違うのは、積んだ更新が走る前に真下がまた書き換わった時だけである。
/// - 上: 持っている形を入力にして、全部を計算し直す (点の保持が効く)。
/// - 水平 d: d の生の接続 s を求める。「接続か」が持っている形と同じで、持っている形が十字でなければ、
///   d だけを s に置き換える。それ以外は、点でない入力で全部を計算し直す。
///
/// 返した状態は、骨格がフラグ 1 を落として書く。NC は出ないので、電力の計算も給電先への通知も起きない
/// (V06 §4.6・§5.3。[実機 fixture dust-reshape-stale-target])。
pub fn update_shape<R: RedstoneRules>(
    lv: &Level<R>,
    pos: Pos,
    state: StateId,
    dir: Dir,
    other: StateId,
) -> StateId {
    match (dir, slot(dir)) {
        (Dir::Down, _) => {
            if can_survive_on(other) {
                state
            } else {
                StateId::AIR
            }
        }
        (_, Some(i)) => {
            let side = connecting_side(lv, pos, dir, can_go_up(lv, pos));
            let mut held = Shape::of(state);
            if side.is_connected() == held.0[i].is_connected() && !held.is_cross() {
                held.0[i] = side;
                held.apply(state)
            } else {
                let mut input = Shape::CROSS;
                input.0[i] = side;
                connection_state(lv, pos, input).apply(state)
            }
        }
        // 上。
        _ => connection_state(lv, pos, Shape::of(state)).apply(state),
    }
}

/// 斜めの通知 (V06 §4.7。RedstoneWireBlock.updateIndirectNeighbourShapes)。
/// 骨格が、書き込みのたびに古い状態と新しい状態について 1 回ずつ呼ぶ。`state` が持っている形を使う。
///
/// 北・東・南・西の順に、接続している方向の横がダストでなければ、横の 1 段下 → 1 段上の順に、
/// ダストがあればそこへ形状更新を送る。方向は「受け手から見てこのダストの側」。
/// 「隣」として渡すのは、受け手からその方向へ 1 マスの位置 (このダストの真下か真上) と、その今の状態。
pub fn update_indirect_shapes<R: RedstoneRules>(
    lv: &mut Level<R>,
    pos: Pos,
    state: StateId,
    flags: UpdateFlags,
    left: i32,
) {
    let held = Shape::of(state);
    for (i, dir) in Dir::HORIZONTAL.into_iter().enumerate() {
        if !held.0[i].is_connected() {
            continue;
        }
        let near = pos.offset(dir);
        if is_wire(lv.get_state(near)) {
            continue;
        }
        for vertical in [Dir::Down, Dir::Up] {
            let receiver = near.offset(vertical);
            if is_wire(lv.get_state(receiver)) {
                let back = dir.opposite();
                let other_pos = receiver.offset(back);
                let other = lv.get_state(other_pos);
                lv.neighbor_shape_changed(back, other, receiver, other_pos, flags, left);
            }
        }
    }
}

/// 信号の出力 (V06 §4.8。RedstoneWireBlock.getSignal)。`dir` は読み手からダストへ向かう方向。
///
/// 1. ダストが信号を出さない間 (ほかのダストが周りを読んでいる間) は 0。
/// 2. 読み手が真上にいれば 0。
/// 3. power が 0 なら 0。
/// 4. 読み手が真下にいれば power。
/// 5. 水平なら、接続を計算し直し、読み手の側が接続していれば power。持っている形は「点か」にしか使わない。
///
/// [実機 fixture dust-cross-powering: 真上のランプは点かず、足元のランプは点く]
pub fn signal<R: RedstoneRules>(lv: &Level<R>, pos: Pos, state: StateId, dir: Dir) -> i32 {
    if !lv.rules.redstone().wire_gives_signal || dir == Dir::Down {
        return 0;
    }
    let power = power_of(state) as i32;
    if power == 0 {
        return 0;
    }
    let Some(toward_reader) = slot(dir.opposite()) else {
        // 垂直で残るのは上 (読み手は真下)。
        return power;
    };
    if connection_state(lv, pos, Shape::of(state)).0[toward_reader].is_connected() {
        power
    } else {
        0
    }
}

/// 強い信号 (V06 §4.8。RedstoneWireBlock.getDirectSignal)。弱い信号と同じ値。
/// ダストは真下と接続方向の導体を強充電している。ほかのダストが読む時だけ隠れる (V06 §5.6)。
pub fn direct_signal<R: RedstoneRules>(lv: &Level<R>, pos: Pos, state: StateId, dir: Dir) -> i32 {
    signal(lv, pos, state, dir)
}

/// 周りのブロックから読む値 (V06 §4.9。RedstoneWireBlock.getBlockSignal)。
/// 読む間だけダストの信号を消す。だから、隣のダスト自身と、ダストだけが充電している導体は値に入らない。
fn block_signal<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos) -> i32 {
    lv.rules.redstone_mut().wire_gives_signal = false;
    let signal = lv.best_neighbor_signal(pos);
    lv.rules.redstone_mut().wire_gives_signal = true;
    signal
}

/// 隣のダストから読む値 (V06 §4.10。RedstoneWireEvaluator.getIncomingWireSignal)。
///
/// 北・東・南・西の順に、横 n について:
/// 1. n がダストなら、その power を読む。接続の形は見ない。
/// 2. n が導体で、自分の真上が導体でなければ、n の真上のダストを読む (上り)。
/// 3. 2 に当たらず、n が導体でなければ、n の真下のダストを読む (下り)。
///
/// 最大から 1 を引く (下限 0)。上りを読む条件に「n が導体」が入るのは、形の規則 (V06 §4.2) と違う所である
/// (V06 §5.1。[実機 fixture wire-step-nonconductor-down・wire-cut-conductor-power])。
fn incoming_wire_signal<R: RedstoneRules>(lv: &Level<R>, pos: Pos) -> i32 {
    let covered = lv.get_state(pos.offset(Dir::Up)).is_conductor();
    let read = |at: Pos| {
        let state = lv.get_state(at);
        if is_wire(state) {
            power_of(state) as i32
        } else {
            0
        }
    };
    let mut best = 0;
    for dir in Dir::HORIZONTAL {
        let near = pos.offset(dir);
        let near_conducts = lv.get_state(near).is_conductor();
        best = best.max(read(near));
        if near_conducts && !covered {
            best = best.max(read(near.offset(Dir::Up)));
        } else if !near_conducts {
            best = best.max(read(near.offset(Dir::Down)));
        }
    }
    (best - 1).max(0)
}

/// 目標の値 (V06 §4.12 の手順 1)。周りのブロックの値が 15 なら 15。そうでなければ、隣のダストの値との大きい方。
fn target_power<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos) -> u8 {
    let from_blocks = block_signal(lv, pos);
    let target = if from_blocks == 15 {
        15
    } else {
        from_blocks.max(incoming_wire_signal(lv, pos))
    };
    debug_assert!((0..=15).contains(&target));
    target as u8
}

/// NC を配る 7 起点を、配る順に返す (V06 §4.12 の手順 4・§4.12.2・V18 の R1)。
/// 自分 → 下・上・北・南・西・東の順に HashSet へ入れ、その反復順に並べる。順は座標で変わる。
fn update_origins(pos: Pos) -> Option<[Pos; 7]> {
    let mut set = JavaHashSet::new();
    set.insert(pos).ok()?;
    for dir in Dir::ALL {
        set.insert(pos.offset(dir)).ok()?;
    }
    let mut out = [pos; 7];
    for (slot, origin) in out.iter_mut().zip(set.iter()) {
        *slot = *origin;
    }
    Some(out)
}

/// 既定の評価器 (V06 §4.12。DefaultRedstoneWireEvaluator.updatePowerStrength)。
/// `state` は渡された状態で、世界の今の状態とは限らない (除去の時は、消えたダストの状態)。
///
/// 1. 目標の値を求める。
/// 2. 渡された状態の power と同じなら終わり。
/// 3. 世界の今の状態が渡された状態とまったく同じ時だけ、power をフラグ 2 で書く (NC は出ず、SU は出る)。
/// 4. 7 起点のそれぞれから、周り 6 マスへ NC を出す (送り元はダスト)。手順 3 で書かなくても出す。
///
/// 更新は骨格のスタックに積むだけで、ここでは再帰しない (V06 §7.3)。
/// 手順 3 の SU は手順 4 の NC より先に積むので、先に走る (V06 §4.12.3)。
/// [実機 fixture instant-wire: 連鎖は同じ tick の中で終わる]
fn update_power_strength<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos, state: StateId) {
    let target = target_power(lv, pos);
    if power_of(state) == target {
        return;
    }
    if lv.get_state(pos) == state {
        lv.set_block(pos, state.with_value(POWER, target), UpdateFlags::CLIENTS);
    }
    match update_origins(pos) {
        Some(origins) => {
            for origin in origins {
                lv.update_neighbors_at(origin, block::REDSTONE_WIRE, None);
            }
        }
        // 7 要素では起きない (V06 §4.12.2)。起きたら黙って順を変えず、状態コードで知らせる。
        None => lv.raise(Status::TREE_BIN),
    }
}

/// NC を受ける (V06 §4.11。RedstoneWireBlock.neighborChanged)。
/// 支えがあれば電力を更新する。無ければ取り除く (フラグ 3。この中で撤去フックが走る)。
/// 既定の評価器の世界では、送り元がダストでも同じに処理する。ダストどうしの NC で電力が伝わる。
pub fn neighbor_changed<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos, state: StateId) {
    if can_survive(lv, pos) {
        update_power_strength(lv, pos, state);
    } else {
        lv.remove_block(pos, false);
    }
}

/// 角の確認 (V06 §4.14。RedstoneWireBlock.checkCornerChangeAt)。
/// 対象がダストなら、対象 → 対象の下・上・北・南・西・東の順に、それぞれを起点に NC を出す。
/// 順は HashSet ではなく、宣言順で固定 (V06 §5.6)。
fn check_corner_change_at<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos) {
    if !is_wire(lv.get_state(pos)) {
        return;
    }
    lv.update_neighbors_at(pos, block::REDSTONE_WIRE, None);
    for dir in Dir::ALL {
        lv.update_neighbors_at(pos.offset(dir), block::REDSTONE_WIRE, None);
    }
}

/// 隣のダストの周りを更新する (V06 §4.14 の手順 3。RedstoneWireBlock.updateNeighborsOfNeighboringWires)。
/// 北・東・南・西の順に横のマスで角の確認をし、続けて同じ順に、横が導体ならその真上で、導体でなければその真下で角の確認をする。
/// ダストを置く・消すと隣のダストの形が変わり、給電先が変わることへの手当てである。
fn update_neighbors_of_neighboring_wires<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos) {
    for dir in Dir::HORIZONTAL {
        check_corner_change_at(lv, pos.offset(dir));
    }
    for dir in Dir::HORIZONTAL {
        let near = pos.offset(dir);
        let vertical = if lv.get_state(near).is_conductor() {
            Dir::Up
        } else {
            Dir::Down
        };
        check_corner_change_at(lv, near.offset(vertical));
    }
}

/// 設置フック (V06 §4.14。RedstoneWireBlock.onPlace)。
/// 古い状態もダストなら何もしない (power や形の書き込みは素通りする)。
///
/// 1. 電力を更新する。
/// 2. 上 → 下の順に、それぞれを起点に NC を出す。
/// 3. 隣のダストの周りを更新する。
///
/// 手順 1 で power が変わると、外側の書き込みは NC も SU も出さない (骨格が読み直して確かめる。V06 §4.14 の落とし穴)。
pub fn on_place<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos, state: StateId, old: StateId) {
    if is_wire(old) {
        return;
    }
    update_power_strength(lv, pos, state);
    for dir in Dir::VERTICAL {
        lv.update_neighbors_at(pos.offset(dir), block::REDSTONE_WIRE, None);
    }
    update_neighbors_of_neighboring_wires(lv, pos);
}

/// 撤去フック (V06 §4.15。RedstoneWireBlock.affectNeighborsAfterRemoval)。
/// 呼ばれる時点で、世界にはもう新しいブロックが入っている。ピストンで動いた時は何もしない。
///
/// 1. 下・上・北・南・西・東の順に、隣を起点に NC を出す。
/// 2. 電力を更新する。渡すのは消えたダストの状態。世界の状態と違うので書き込みは起きず、
///    目標が消えたダストの power と違う時だけ 7 起点の NC が出る。
/// 3. 隣のダストの周りを更新する。
pub fn on_remove<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos, old: StateId, moved: bool) {
    if moved {
        return;
    }
    for dir in Dir::ALL {
        lv.update_neighbors_at(pos.offset(dir), block::REDSTONE_WIRE, None);
    }
    update_power_strength(lv, pos, old);
    update_neighbors_of_neighboring_wires(lv, pos);
}

/// 右クリック (V06 §4.16。RedstoneWireBlock.useWithoutItem)。建築の権限はあるものとする。
///
/// 持っている形が十字なら点を、点なら十字の基準を候補にして、接続を計算し直す。
/// 十字でも点でもなければ何もしない。結果が今と違えばフラグ 3 で書き、続けて、接続の有無が変わった方向の
/// 横が導体なら、そこを起点に (このダストへ戻る方向を除いて) NC を出す。power は変わらない。
/// 点にできるのは、生の接続が 0 本のダストだけである。
pub fn use_block<R: RedstoneRules>(lv: &mut Level<R>, pos: Pos, state: StateId) -> bool {
    let held = Shape::of(state);
    let candidate = if held.is_cross() {
        Shape::DOT
    } else if held.is_dot() {
        Shape::CROSS
    } else {
        return false;
    };
    let next = connection_state(lv, pos, candidate);
    if next == held {
        return false;
    }
    lv.set_block(pos, next.apply(state), UpdateFlags::ALL);
    for (i, dir) in Dir::HORIZONTAL.into_iter().enumerate() {
        let near = pos.offset(dir);
        if held.0[i].is_connected() != next.0[i].is_connected() && lv.is_conductor(near) {
            // 送り元は、起点の導体ではなくダスト (V06 §4.11)。
            lv.update_neighbors_at_except(near, block::REDSTONE_WIRE, dir.opposite(), None);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Bench, at, level, put, stone};
    use rse_core::{Input, InputSection};

    fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos::new(x, y, z)
    }

    fn parse(text: &str) -> StateId {
        StateId::parse(text).unwrap()
    }

    /// 石の床 (y=0) を、x と z の範囲に敷く。
    fn floor(
        lv: &mut Level<Bench>,
        xs: core::ops::RangeInclusive<i32>,
        zs: core::ops::RangeInclusive<i32>,
    ) {
        for x in xs {
            for z in zs.clone() {
                put(lv, p(x, 0, z), stone());
            }
        }
    }

    /// 設置の時の状態をフラグ 11 で書く (V06 §4.14)。素子の関数だけを見るための近道で、置く先が置き換え可能かと
    /// 支えを確かめない。本物の手順は置く入力 (rse-core の input) にある。
    fn place_wire(lv: &mut Level<Bench>, pos: Pos) {
        let state = placement_state(lv, pos);
        lv.set_block(pos, state, UpdateFlags::ALL_IMMEDIATE);
    }

    /// 形だけを "N E S W" の順の文字 (u・s・-) で読む。
    fn shape_at(lv: &Level<Bench>, pos: Pos) -> String {
        Shape::of(lv.get_state(pos))
            .0
            .iter()
            .map(|s| match s {
                Side::Up => 'u',
                Side::Side => 's',
                Side::None => '-',
            })
            .collect()
    }

    fn power_at(lv: &Level<Bench>, pos: Pos) -> u8 {
        let state = lv.get_state(pos);
        assert!(is_wire(state), "{pos:?} はダストでない: {state}");
        power_of(state)
    }

    fn toggle(lv: &mut Level<Bench>, pos: Pos) {
        assert!(lv.input(InputSection::A, Input::Use { pos }).accepted);
    }

    const LEVER_OFF: &str = "lever[face=floor,facing=north,powered=false]";

    // プロパティの添字と、接続の値の番号が状態表と合っている。
    #[test]
    fn property_slots_match_the_table() {
        let wire = block::REDSTONE_WIRE;
        assert_eq!(wire.prop("east"), Some(EAST));
        assert_eq!(wire.prop("north"), Some(NORTH));
        assert_eq!(wire.prop("power"), Some(POWER));
        assert_eq!(wire.prop("south"), Some(SOUTH));
        assert_eq!(wire.prop("west"), Some(WEST));
        let s = parse("redstone_wire[east=up,north=side,power=7,south=none,west=up]");
        assert_eq!(
            Shape::of(s),
            Shape([Side::Side, Side::Up, Side::None, Side::Up])
        );
        assert_eq!(power_of(s), 7);
        assert_eq!(
            Shape::of(s).apply(wire.default_state().with_value(POWER, 7)),
            s
        );
        assert_eq!(
            Dir::HORIZONTAL,
            [Dir::North, Dir::East, Dir::South, Dir::West]
        );
    }

    // 孤立したダストは、置くと十字になる。点は点のまま (V06 §4.3 の表・§4.4)。
    #[test]
    fn isolated_wire_is_a_cross_and_a_dot_stays_a_dot() {
        let mut lv = level();
        floor(&mut lv, 0..=2, 0..=2);
        let c = p(1, 1, 1);
        assert_eq!(connection_state(&lv, c, Shape::CROSS), Shape::CROSS);
        assert_eq!(connection_state(&lv, c, Shape::DOT), Shape::DOT);
        place_wire(&mut lv, c);
        assert_eq!(shape_at(&lv, c), "ssss");
    }

    // 接続 1 本は直線になる。向かい合わせ・角・3 本はそのまま (V06 §4.3 の表)。
    #[test]
    fn connection_shapes_follow_the_table() {
        let c = p(2, 1, 2);
        let lever = parse(LEVER_OFF);
        // (接続先を置く方向, 期待する形 N E S W)
        let cases: [(&[Dir], &str); 6] = [
            (&[Dir::West], "-s-s"),
            (&[Dir::North], "s-s-"),
            (&[Dir::West, Dir::East], "-s-s"),
            (&[Dir::North, Dir::East], "ss--"),
            (&[Dir::North, Dir::East, Dir::South], "sss-"),
            (&[Dir::North, Dir::East, Dir::South, Dir::West], "ssss"),
        ];
        for (dirs, expect) in cases {
            let mut lv = level();
            floor(&mut lv, 0..=4, 0..=4);
            for d in dirs {
                put(&mut lv, c.offset(*d), lever);
            }
            place_wire(&mut lv, c);
            assert_eq!(shape_at(&lv, c), expect, "{dirs:?}");
            // 点として置いてあっても、生の接続があれば点にならない。
            assert_ne!(connection_state(&lv, c, Shape::DOT), Shape::DOT);
        }
    }

    // 導体でも信号源でもない隣 (石・ランプ) には接続しない。信号源 (レバー) とダストには接続する (V06 §4.1)。
    #[test]
    fn connects_to_sources_and_wires_only() {
        let mut lv = level();
        floor(&mut lv, 0..=4, 0..=4);
        let c = p(2, 1, 2);
        put(&mut lv, c.offset(Dir::North), stone());
        put(
            &mut lv,
            c.offset(Dir::East),
            parse("redstone_lamp[lit=false]"),
        );
        assert_eq!(connection_state(&lv, c, Shape::DOT), Shape::DOT);
        put(
            &mut lv,
            c.offset(Dir::South),
            block::REDSTONE_WIRE.default_state(),
        );
        assert_eq!(
            connection_state(&lv, c, Shape::DOT),
            Shape([Side::Side, Side::None, Side::Side, Side::None])
        );
    }

    // 段差: 石の上のダストへは up で、下のダストへは side で接続する。関係は対称 (V06 §4.2)。
    // 下の段の真上に導体があると、上りも下りも切れる。
    #[test]
    fn step_connections_go_up_and_down() {
        let mut lv = level();
        floor(&mut lv, 0..=3, 0..=2);
        let low = p(1, 1, 1);
        let step = p(2, 1, 1);
        let high = p(2, 2, 1);
        put(&mut lv, step, stone());
        place_wire(&mut lv, low);
        place_wire(&mut lv, high);
        // 低い方は東へ up。1 本なので西へ延びる。高い方は西へ side (下り)。1 本なので東へ延びる。
        assert_eq!(shape_at(&lv, low), "-u-s");
        assert_eq!(shape_at(&lv, high), "-s-s");
        // 低い方の真上を導体でふさぐと、両方とも切れる。
        // 高い方は、西の隣が導体になるので下りを見ない。
        put(&mut lv, low.offset(Dir::Up), stone());
        assert_eq!(connection_state(&lv, low, Shape::DOT), Shape::DOT);
        assert_eq!(connection_state(&lv, high, Shape::DOT), Shape::DOT);
    }

    // 上の段にダストを置くと、下の段のダストは斜めの通知で形を直す (V06 §4.7)。
    // 取り除くと、また斜めの通知で元へ戻る (古い状態が持っていた形で配る)。
    #[test]
    fn diagonal_wire_gets_shape_updates() {
        let mut lv = level();
        floor(&mut lv, 0..=3, 0..=2);
        let low = p(1, 1, 1);
        let high = p(2, 2, 1);
        put(&mut lv, p(2, 1, 1), stone());
        place_wire(&mut lv, low);
        assert_eq!(shape_at(&lv, low), "ssss");
        place_wire(&mut lv, high);
        assert_eq!(shape_at(&lv, low), "-u-s");
        lv.remove_block(high, false);
        assert_eq!(shape_at(&lv, low), "ssss");
    }

    // 電力はダスト 1 本ごとに 1 減る。15 本先で 1、16 本先で 0 (V06 §3.5・§4.10・§4.12)。
    // 切ると、全部が同じ入力の中で 0 に戻る (V06 §4.12.3)。
    #[test]
    fn power_decays_by_one_per_wire() {
        let mut lv = level();
        floor(&mut lv, 0..=17, 0..=0);
        let lever = p(0, 1, 0);
        put(&mut lv, lever, parse(LEVER_OFF));
        for x in 1..=17 {
            place_wire(&mut lv, p(x, 1, 0));
        }
        toggle(&mut lv, lever);
        for x in 1..=17 {
            assert_eq!(power_at(&lv, p(x, 1, 0)) as i32, (16 - x).max(0), "x={x}");
        }
        toggle(&mut lv, lever);
        for x in 1..=17 {
            assert_eq!(power_at(&lv, p(x, 1, 0)), 0, "x={x}");
        }
        assert!(lv.take_status().is_ok());
    }

    // 入ったレバーの横に置いた瞬間に給電される (設置フックが電力を更新する。V06 §4.14)。
    // 取り除くと、その先のダストは 0 に戻る (撤去フック。V06 §4.15)。
    #[test]
    fn placing_and_removing_updates_power() {
        let mut lv = level();
        floor(&mut lv, 0..=3, 0..=0);
        put(
            &mut lv,
            p(0, 1, 0),
            parse("lever[face=floor,facing=north,powered=true]"),
        );
        place_wire(&mut lv, p(2, 1, 0));
        assert_eq!(power_at(&lv, p(2, 1, 0)), 0);
        place_wire(&mut lv, p(1, 1, 0));
        assert_eq!(power_at(&lv, p(1, 1, 0)), 15);
        assert_eq!(power_at(&lv, p(2, 1, 0)), 14);
        lv.remove_block(p(1, 1, 0), false);
        assert_eq!(power_at(&lv, p(2, 1, 0)), 0);
        // 隣が消えたので、残ったダストは十字へ戻る (水平の形状更新で接続の有無が変わった)。
        assert_eq!(shape_at(&lv, p(2, 1, 0)), "ssss");
    }

    // 信号の出力 (V06 §4.8 の表): 真下へは power、真上へは 0、水平は接続している側だけ。
    // 接続は持っている形ではなく、計算し直した形で決まる (V06 §5.2)。
    #[test]
    fn signal_goes_down_and_to_connected_sides() {
        let mut lv = level();
        floor(&mut lv, 0..=2, 0..=2);
        let c = p(1, 1, 1);
        // 西にレバーがあるので、計算し直した形は東西の直線。持っている形は、わざと南北にしておく。
        put(&mut lv, c.offset(Dir::West), parse(LEVER_OFF));
        let held = parse("redstone_wire[east=none,north=side,power=9,south=side,west=none]");
        put(&mut lv, c, held);
        // dir は読み手からダストへ向かう方向。
        assert_eq!(signal(&lv, c, held, Dir::Up), 9); // 読み手は真下
        assert_eq!(signal(&lv, c, held, Dir::Down), 0); // 読み手は真上
        assert_eq!(signal(&lv, c, held, Dir::West), 9); // 読み手は東
        assert_eq!(signal(&lv, c, held, Dir::East), 9); // 読み手は西
        assert_eq!(signal(&lv, c, held, Dir::North), 0); // 読み手は南
        assert_eq!(signal(&lv, c, held, Dir::South), 0); // 読み手は北
        for dir in Dir::ALL {
            assert_eq!(direct_signal(&lv, c, held, dir), signal(&lv, c, held, dir));
            // power 0 はどこへも 0。
            assert_eq!(signal(&lv, c, held.with_value(POWER, 0), dir), 0);
        }
        // 点は、水平へは出さない。真下へは出す。
        lv.remove_block(c.offset(Dir::West), false);
        let dot = Shape::DOT.apply(held);
        put(&mut lv, c, dot);
        assert_eq!(signal(&lv, c, dot, Dir::Up), 9);
        for dir in Dir::HORIZONTAL {
            assert_eq!(signal(&lv, c, dot, dir), 0);
        }
    }

    // ダストが充電した導体は、ほかのダストに給電しない。ランプは点く (V06 §4.8・§4.9)。
    // レバー → ダスト a → 石 → (石の先に) ダスト b とランプ。
    #[test]
    fn wire_charged_conductor_powers_lamps_but_not_wires() {
        let mut lv = level();
        floor(&mut lv, 0..=4, 0..=1);
        let lever = p(0, 1, 0);
        let (a, block_pos, b) = (p(1, 1, 0), p(2, 1, 0), p(3, 1, 0));
        let lamp = p(2, 1, 1);
        put(&mut lv, lever, parse(LEVER_OFF));
        put(&mut lv, block_pos, stone());
        put(&mut lv, lamp, parse("redstone_lamp[lit=false]"));
        place_wire(&mut lv, a);
        place_wire(&mut lv, b);
        toggle(&mut lv, lever);
        assert_eq!(power_at(&lv, a), 15);
        // 石は a に強充電されている。
        assert_eq!(lv.direct_signal_to(block_pos), 15);
        assert_eq!(power_at(&lv, b), 0);
        assert_eq!(at(&lv, lamp), "redstone_lamp[lit=true]");
        // 計算が終わると、ダストはまた信号を出す。信号源の印も戻る (V06 §4.8)。
        assert!(lv.rules.redstone().wire_gives_signal());
        assert!(lv.is_signal_source(lv.get_state(a)));
    }

    // 段差の電力: 石の段では、上りも下りも読む (V06 §4.10)。
    #[test]
    fn power_climbs_and_descends_stone_steps() {
        let mut lv = level();
        floor(&mut lv, 0..=4, 0..=0);
        let lever = p(0, 1, 0);
        put(&mut lv, lever, parse(LEVER_OFF));
        put(&mut lv, p(2, 1, 0), stone());
        put(&mut lv, p(3, 1, 0), stone());
        put(&mut lv, p(3, 2, 0), stone());
        for pos in [p(1, 1, 0), p(2, 2, 0), p(3, 3, 0)] {
            place_wire(&mut lv, pos);
        }
        toggle(&mut lv, lever);
        assert_eq!(power_at(&lv, p(1, 1, 0)), 15);
        assert_eq!(power_at(&lv, p(2, 2, 0)), 14);
        assert_eq!(power_at(&lv, p(3, 3, 0)), 13);
        // 一番下の真上を導体でふさぐと、上の段は読まなくなる (V06 §4.10 の「真上が導体」)。
        // 上の段からも、横 (2,1,0) が導体なので下りを読まない。ふさいだ石の NC で計算し直す。
        lv.set_block(p(1, 2, 0), stone(), UpdateFlags::ALL);
        assert_eq!(power_at(&lv, p(1, 1, 0)), 15);
        assert_eq!(power_at(&lv, p(2, 2, 0)), 0);
        assert_eq!(power_at(&lv, p(3, 3, 0)), 0);
    }

    // 支え (V06 §4.5): 下がフラグ 3 で消えると NC の経路で、フラグ 2 で消えると SU の経路で消える。
    #[test]
    fn wire_pops_when_support_goes_away() {
        for flags in [UpdateFlags::ALL, UpdateFlags::CLIENTS] {
            let mut lv = level();
            floor(&mut lv, 0..=2, 0..=0);
            place_wire(&mut lv, p(1, 1, 0));
            assert!(can_survive(&lv, p(1, 1, 0)));
            lv.set_block(p(1, 0, 0), StateId::AIR, flags);
            assert_eq!(at(&lv, p(1, 1, 0)), "air", "フラグ {}", flags.0);
        }
        // ダストやレバーの上には残れない (V05 §4.7.5)。
        let mut lv = level();
        floor(&mut lv, 0..=0, 0..=0);
        put(&mut lv, p(0, 1, 0), block::REDSTONE_WIRE.default_state());
        assert!(!can_survive(&lv, p(0, 2, 0)));
        put(&mut lv, p(0, 1, 0), parse(LEVER_OFF));
        assert!(!can_survive(&lv, p(0, 2, 0)));
    }

    // 下からの形状更新は、運ばれた状態で支えを判定する。世界の今の真下は読まない (V06 §4.6)。
    // 2 つが食い違うのは、積んだ更新が走る前に真下がまた書き換わった時 (V04 §5.5)。
    #[test]
    fn shape_update_from_below_uses_the_carried_state() {
        const LINE: &str = "redstone_wire[east=side,north=none,power=0,south=none,west=side]";
        let (wire, below) = (p(1, 1, 0), p(1, 0, 0));
        let line = parse(LINE);

        // 真下を「石 → 空気 → 石」と続けて書いた時の 1 件目。世界は石に戻っているが、運ばれたのは空気。
        let mut lv = level();
        put(&mut lv, below, stone());
        put(&mut lv, wire, line);
        assert!(can_survive(&lv, wire));
        assert_eq!(
            update_shape(&lv, wire, line, Dir::Down, StateId::AIR),
            StateId::AIR,
            "支えない状態が運ばれたら、世界の真下が石でも消える"
        );
        // 支えにならない物はどれも同じ (ダスト・レバーの上には残れない。V05 §4.7.5)。
        assert_eq!(
            update_shape(&lv, wire, line, Dir::Down, parse(LEVER_OFF)),
            StateId::AIR
        );

        // 真下を「石 → 空気」と続けて書いた時の 1 件目。世界はもう空気だが、運ばれたのは石。
        let mut lv = level();
        put(&mut lv, wire, line);
        assert!(!can_survive(&lv, wire));
        assert_eq!(
            update_shape(&lv, wire, line, Dir::Down, stone()),
            line,
            "支える状態が運ばれたら、世界の真下が空気でも残る"
        );
        // 2 件目 (空気) で消える。
        assert_eq!(
            update_shape(&lv, wire, line, Dir::Down, StateId::AIR),
            StateId::AIR
        );

        // 下の腕は形に触れない。持っている形が実情とずれていても、そのまま返す。
        let mut lv = level();
        put(&mut lv, below, stone());
        put(&mut lv, wire, line);
        put(&mut lv, p(1, 1, 1), block::REDSTONE_WIRE.default_state());
        assert_eq!(update_shape(&lv, wire, line, Dir::Down, stone()), line);
    }

    // 骨格を通した時も、書いた状態が運ばれて届く。1 回の書き込みでは、運ばれた状態と世界は同じ (V06 §4.6)。
    #[test]
    fn shape_update_from_below_through_the_queue() {
        // フラグ 2 だけ (NC を出さない) で真下を別の支えに書き換える。形状更新の経路だけが走り、残る。
        let mut lv = level();
        floor(&mut lv, 0..=2, 0..=0);
        place_wire(&mut lv, p(1, 1, 0));
        let before = at(&lv, p(1, 1, 0));
        lv.set_block(
            p(1, 0, 0),
            parse("redstone_lamp[lit=false]"),
            UpdateFlags::CLIENTS,
        );
        assert_eq!(at(&lv, p(1, 1, 0)), before);
        // 支えにならない物 (レバー) に書き換えると、形状更新の経路で消える。
        lv.set_block(p(1, 0, 0), parse(LEVER_OFF), UpdateFlags::CLIENTS);
        assert_eq!(at(&lv, p(1, 1, 0)), "air");
    }

    // 7 起点の順 (V06 §4.12.2 の表)。座標から計算で求める。原点の近くは組の規則から外れる。
    #[test]
    fn update_origins_follow_hash_set_order() {
        use Dir::*;
        // None は p 自身。
        type Case = ((i32, i32, i32), [Option<Dir>; 7]);
        let me = None;
        let cases: [Case; 7] = [
            (
                (0, 1, 0),
                [
                    Some(Down),
                    Some(South),
                    Some(East),
                    Some(North),
                    Some(Up),
                    Some(West),
                    me,
                ],
            ),
            (
                (1, 1, 0),
                [
                    me,
                    Some(North),
                    Some(Down),
                    Some(South),
                    Some(East),
                    Some(Up),
                    Some(West),
                ],
            ),
            (
                (2, 1, 0),
                [
                    Some(Up),
                    Some(West),
                    me,
                    Some(Down),
                    Some(South),
                    Some(East),
                    Some(North),
                ],
            ),
            (
                (0, 0, 0),
                [
                    me,
                    Some(North),
                    Some(West),
                    Some(South),
                    Some(East),
                    Some(Down),
                    Some(Up),
                ],
            ),
            (
                (5, 64, 5),
                [
                    Some(Up),
                    Some(North),
                    Some(West),
                    me,
                    Some(Down),
                    Some(South),
                    Some(East),
                ],
            ),
            (
                (-3, 64, -7),
                [
                    Some(Down),
                    Some(South),
                    Some(East),
                    me,
                    Some(Up),
                    Some(North),
                    Some(West),
                ],
            ),
            (
                (100, 70, 200),
                [
                    me,
                    Some(Down),
                    Some(South),
                    Some(East),
                    Some(Up),
                    Some(North),
                    Some(West),
                ],
            ),
        ];
        for ((x, y, z), order) in cases {
            let pos = p(x, y, z);
            let expect = order.map(|d| d.map_or(pos, |d| pos.offset(d)));
            assert_eq!(update_origins(pos), Some(expect), "{pos:?}");
        }
    }

    // 電力が変わると、7 起点 × 6 方向 = 42 件の NC が、起点の順 × NC の順で出る (V06 §4.12 の手順 5)。
    // 周りに反応する物が無い 1 本だけのダストで数える。
    #[test]
    fn power_change_sends_42_updates_in_origin_order() {
        let mut lv = level();
        let c = p(5, 64, 5);
        put(&mut lv, c.offset(Dir::Down), stone());
        // 点として置き、強充電された石を足元に作る代わりに、状態を直に書いて NC を 1 件送る。
        let dot = block::REDSTONE_WIRE.default_state().with_value(POWER, 6);
        put(&mut lv, c, dot);
        lv.rules.nc_log.clear();
        lv.neighbor_changed(c, block::STONE, None);
        // 周りに信号が無いので 0 になる。
        assert_eq!(power_at(&lv, c), 0);
        let from_wire: Vec<Pos> = lv
            .rules
            .nc_log
            .iter()
            .filter(|(_, src)| *src == block::REDSTONE_WIRE)
            .map(|(pos, _)| *pos)
            .collect();
        let mut expect = Vec::new();
        for origin in update_origins(c).unwrap() {
            for dir in Dir::NC_ORDER {
                expect.push(origin.offset(dir));
            }
        }
        assert_eq!(from_wire.len(), 42);
        assert_eq!(from_wire, expect);
        // 自分へは 6 回戻ってくる (V06 §4.12.1)。
        assert_eq!(from_wire.iter().filter(|pos| **pos == c).count(), 6);
    }

    // 挟まれたダストを消しても、目標が変わらなければ 7 起点の NC は出ない (V06 §4.15 の例)。
    // 14・13・12 の 13 を消す。隣のダストは手順 1 の NC で計算し直す。
    #[test]
    fn removal_recomputes_neighbors() {
        let mut lv = level();
        floor(&mut lv, 0..=4, 0..=0);
        let lever = p(0, 1, 0);
        put(&mut lv, lever, parse(LEVER_OFF));
        for x in 1..=4 {
            place_wire(&mut lv, p(x, 1, 0));
        }
        toggle(&mut lv, lever);
        assert_eq!(power_at(&lv, p(3, 1, 0)), 13);
        lv.remove_block(p(3, 1, 0), false);
        assert_eq!(power_at(&lv, p(1, 1, 0)), 15);
        assert_eq!(power_at(&lv, p(2, 1, 0)), 14);
        assert_eq!(power_at(&lv, p(4, 1, 0)), 0);
    }

    // 右クリック (V06 §4.16): 孤立した十字 ⇔ 点。power は変わらない。
    // 生の接続があるダストは点にできない。十字でも点でもないダストは何もしない。
    #[test]
    fn use_toggles_cross_and_dot() {
        let mut lv = level();
        floor(&mut lv, 0..=4, 0..=2);
        let c = p(1, 1, 1);
        place_wire(&mut lv, c);
        let use_it = Input::Use { pos: c };
        assert!(lv.input(InputSection::A, use_it).accepted);
        assert_eq!(shape_at(&lv, c), "----");
        assert!(lv.input(InputSection::A, use_it).accepted);
        assert_eq!(shape_at(&lv, c), "ssss");
        // 隣にダストを置くと直線になり、右クリックは効かない。
        place_wire(&mut lv, c.offset(Dir::East));
        assert_eq!(shape_at(&lv, c), "-s-s");
        assert!(!lv.input(InputSection::A, use_it).accepted);
        assert_eq!(shape_at(&lv, c), "-s-s");
    }

    // 点にすると、水平の給電が止まる。横の導体の先のランプへは、手順 6 の NC が届く (V06 §4.16)。
    #[test]
    fn use_updates_blocks_behind_conductors() {
        let mut lv = level();
        floor(&mut lv, 0..=3, 0..=2);
        let c = p(1, 1, 1);
        let block_pos = c.offset(Dir::East);
        let lamp = block_pos.offset(Dir::East);
        put(&mut lv, block_pos, stone());
        put(&mut lv, lamp, parse("redstone_lamp[lit=true]"));
        // 十字で power 15 のダスト。東の石を強充電している。
        put(
            &mut lv,
            c,
            Shape::CROSS.apply(block::REDSTONE_WIRE.default_state().with_value(POWER, 15)),
        );
        assert!(lv.has_neighbor_signal(lamp));
        assert!(lv.input(InputSection::A, Input::Use { pos: c }).accepted);
        assert_eq!(shape_at(&lv, c), "----");
        assert_eq!(power_at(&lv, c), 15);
        assert!(!lv.has_neighbor_signal(lamp));
        // ランプは石の先にあり、ダストの隣ではない。届いたのは手順 6 の NC である。
        assert_eq!(lv.scheduled_tick_count(), 1);
    }
}
