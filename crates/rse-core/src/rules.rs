//! 振る舞いの振り分けの口 (19 §2.4・docs/architecture.md §3)。
//!
//! 骨格は素子を知らない。素子のフックは、この trait の関連関数として呼ぶ。
//! 実装は rse-engine に 1 つだけあり、状態の `Behavior` を match して族の関数を呼ぶ。
//! 呼び出しはジェネリクスで静的に決まるので、間接呼び出しは無い。
//!
//! フックは `&mut Level` の全体を受け、再入してよい (フックの中から書き込み・更新・予約を頼める)。
//! フックを呼ぶ条件と順は骨格が持つ。素子の側に置かない (V04 §7.3)。
//!
//! 既定の実装は置かない。振り分けの抜けを、実装する側のコンパイルで見つけるためである。
//! 例外は [`Rules::is_signal_source`] だけで、表の値を返す。

use crate::level::Level;
use crate::set_block::UpdateFlags;
use crate::state::{BlockId, StateId, flag};
use crate::update::Orientation;
use rse_java::{Dir, Pos};

/// 置いた時の状態の答え ([`Rules::placement_state`])。
///
/// 「置けない」と「規則がまだ無い」を分ける。前は実機でも何も起きないので状態コードを立てず、
/// 後は実機と違う結果になりうるので、骨格が UNIMPLEMENTED を立てる。どちらの時も、骨格は何も書かない。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// この状態を置く。
    State(StateId),
    /// 置けない。実機でも何も起きない。
    Refused,
    /// 置いた時の規則がまだ無い。
    Unimplemented,
}

/// 素子の振る舞い。型そのものが振り分けの表で、値は素子が世界ごとに持つ状態
/// (`Level::rules`) を入れる所として使う。
pub trait Rules: Sized {
    /// 近隣更新 (NC) を受ける (V04 §4.5。BlockBehaviour.neighborChanged)。
    /// 受け手は空気でも呼ばれる。`src` は送り元のブロックの種類で、座標は運ばない。
    fn neighbor_changed(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        src: BlockId,
        ori: Option<Orientation>,
        moved: bool,
    );

    /// 形状更新 (SU) を受け、新しい状態を返す (V04 §4.6。BlockBehaviour.updateShape)。
    /// 変えないなら `state` をそのまま返す。空気を返すと、骨格がそのブロックを壊す。
    /// `dir` は受け手から見た相手の方向。`other` は SU を積んだ時点の相手の状態で、今の状態とは限らない (V04 §5.5)。
    fn update_shape(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        dir: Dir,
        other_pos: Pos,
        other: StateId,
    ) -> StateId;

    /// 間接の形状更新 (V04 §4.6。BlockBehaviour.updateIndirectNeighbourShapes)。
    /// 書き込みのたびに、古い状態と新しい状態について 1 回ずつ呼ぶ。中身があるのはダストだけ (V06 §4.7)。
    fn update_indirect_shapes(
        lv: &mut Level<Self>,
        pos: Pos,
        state: StateId,
        flags: UpdateFlags,
        left: i32,
    );

    /// 設置フック (V04 §4.2 の手順 9。BlockBehaviour.onPlace)。
    /// 同じブロックでプロパティだけ変わった時も呼ぶ。`old` は書き込みの前の状態。
    fn on_place(lv: &mut Level<Self>, pos: Pos, state: StateId, old: StateId, moved: bool);

    /// 撤去フック (V04 §4.2 の手順 7。BlockBehaviour.affectNeighborsAfterRemoval)。
    /// 呼ぶ時点で、世界にはもう新しい状態が入っている。`old` は撤去された状態。
    fn on_remove(lv: &mut Level<Self>, pos: Pos, old: StateId, moved: bool);

    /// 予約 tick の配送 (V02 §4.6。BlockBehaviour.tick)。`state` は今の状態。
    fn scheduled_tick(lv: &mut Level<Self>, pos: Pos, state: StateId);

    /// ブロックイベントの配送 (V03 §4.3。BlockBehaviour.triggerEvent)。`state` は今の状態。
    /// 戻り値は trace 用で、挙動には使わない (V03 §7.2)。
    fn block_event(lv: &mut Level<Self>, pos: Pos, state: StateId, a: i32, b: i32) -> bool;

    /// 入力「使う」(右クリック) を受ける (V01 §4.10)。何かしたら真。
    fn use_block(lv: &mut Level<Self>, pos: Pos, state: StateId) -> bool;

    /// プレイヤーが置く時の状態を決める (V06 §4.4・V11 §4.1・V09 §3.1。BlockBehaviour.getStateForPlacement に当たる)。
    /// `requested` は入力が運んだ状態で、置くブロックと、プレイヤーが選ぶ値 (レバーの face・facing) を運ぶ。
    /// 周りで決まる値 (ダストの 4 方向と power・ランプの lit) は読まずに、世界から決める。
    ///
    /// 世界を書き換えない。置く手順 ([`crate::input`]) は、ここと [`Rules::can_survive`] を済ませてから 1 回だけ書く。
    /// 断る時に何も書かないことを、引数の型で守る。状態コードも立てられないので、規則が無い時は
    /// [`Placement::Unimplemented`] を返し、骨格に立てさせる。
    fn placement_state(lv: &Level<Self>, pos: Pos, requested: StateId) -> Placement;

    /// `state` が `pos` に居られるか (V07 §4.13・V06 §4.5・V09 §4.1。BlockBehaviour.canSurvive に当たる)。
    /// 置く手順が、[`Rules::placement_state`] の返した状態について呼ぶ。偽なら置かない。世界を書き換えない。
    ///
    /// 形状更新や近隣更新で支えを確かめ直す所は、素子の関数が自分で持つ (どの更新で・何を見て確かめるかが
    /// 素子ごとに違う。V06 §4.5・V09 §4.1)。この口は通らない。
    fn can_survive(lv: &Level<Self>, pos: Pos, state: StateId) -> bool;

    /// 弱い信号 (V05 §2.1。BlockBehaviour.getSignal)。
    /// `dir` は読み手から源へ向かう向き (V05 §2.2)。世界を書き換えない。
    fn signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32;

    /// 強い信号 (V05 §2.1。BlockBehaviour.getDirectSignal)。向きの約束は弱い信号と同じ。
    fn direct_signal(lv: &Level<Self>, pos: Pos, state: StateId, dir: Dir) -> i32;

    /// 信号源の印 (V05 §2.1)。既定は表の値。
    /// ダストは評価中だけ偽になるので、ダストの族を持つ実装が上書きする (V05 §7.2)。
    fn is_signal_source(lv: &Level<Self>, state: StateId) -> bool {
        let _ = lv;
        state.has(flag::SIGNAL_SOURCE)
    }
}
