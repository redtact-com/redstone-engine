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
