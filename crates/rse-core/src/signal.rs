//! 信号の問い合わせ (V05 §2.3・§4.1)。
//!
//! - 信号は 2 系統ある。弱い信号は隣の読み手へ届く。強い信号は隣の導体へ届き、
//!   導体はそれを自分の信号として周りの読み手に見せる (V05 §2.1)。
//! - 向きは「読み手から源へ向かう向き」で渡す (V05 §2.2)。
//! - 問い合わせは世界を書き換えない。順は早期終了の位置にしか効かないが、宣言順に揃える (V05 §7.2)。
//!
//! 信号は 0〜15 だが、型は i32 のまま持つ (素子のフックが返す値を丸めない)。
//! ダイオードの側面入力 (getControlInputSignal) は、ダイオードの PR で足す。

use crate::level::Level;
use crate::rules::Rules;
use crate::state::StateId;
use rse_java::{Dir, Pos};

/// 信号の最大 (V05 §2.1)。
pub const MAX_SIGNAL: i32 = 15;

impl<R: Rules> Level<R> {
    /// 源 `pos` が、読み手へ出す強い信号 (V05 §4.1。SignalGetter.getDirectSignal)。
    pub fn direct_signal(&self, pos: Pos, dir: Dir) -> i32 {
        R::direct_signal(self, pos, self.world.get(pos), dir)
    }

    /// 位置 `pos` が周り 6 マスから受ける強い信号の最大 (V05 §2.3。SignalGetter.getDirectSignalTo)。
    /// 下・上・北・南・西・東の順に見て、15 以上になったらその場で返す。
    /// 6 マスには、問い合わせてきた読み手自身も入る (V05 §5 の 3)。
    pub fn direct_signal_to(&self, pos: Pos) -> i32 {
        let mut best = 0;
        for dir in Dir::ALL {
            best = best.max(self.direct_signal(pos.offset(dir), dir));
            if best >= MAX_SIGNAL {
                return best;
            }
        }
        best
    }

    /// 源 `pos` が、読み手へ見せる信号 (V05 §2.3。SignalGetter.getSignal)。
    /// 源の弱い信号を読み、源が導体なら、源が受けている強い信号との大きい方を返す。減衰は無い。
    pub fn signal(&self, pos: Pos, dir: Dir) -> i32 {
        let state = self.world.get(pos);
        let weak = R::signal(self, pos, state, dir);
        if state.is_conductor() {
            weak.max(self.direct_signal_to(pos))
        } else {
            weak
        }
    }

    /// 源 `pos` が読み手へ信号を見せているか (V05 §4.1。SignalGetter.hasSignal)。
    pub fn has_signal(&self, pos: Pos, dir: Dir) -> bool {
        self.signal(pos, dir) > 0
    }

    /// 位置 `pos` が、6 方向のどれかから信号を受けているか (V05 §4.1。SignalGetter.hasNeighborSignal)。
    /// 下・上・北・南・西・東の順に見て、最初に真になった所で返す。
    pub fn has_neighbor_signal(&self, pos: Pos) -> bool {
        Dir::ALL
            .into_iter()
            .any(|dir| self.signal(pos.offset(dir), dir) > 0)
    }

    /// 位置 `pos` が 6 方向から受ける信号の最大 (V05 §4.1。SignalGetter.getBestNeighborSignal)。
    /// 15 以上が出たら、その場で 15 を返す。
    pub fn best_neighbor_signal(&self, pos: Pos) -> i32 {
        let mut best = 0;
        for dir in Dir::ALL {
            let signal = self.signal(pos.offset(dir), dir);
            if signal >= MAX_SIGNAL {
                return MAX_SIGNAL;
            }
            best = best.max(signal);
        }
        best
    }

    /// 導体か (V05 §4.4)。
    pub fn is_conductor(&self, pos: Pos) -> bool {
        self.world.get(pos).is_conductor()
    }

    /// 信号源の印 (V05 §2.1)。ダストは評価中だけ偽になるので、表を直に引かずに Rules を通す (V05 §7.2)。
    pub fn is_signal_source(&self, state: StateId) -> bool {
        R::is_signal_source(self, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::block;
    use crate::testing::{level, powered_lever, wire_with_power};
    use rse_java::Dir::*;

    // テスト用の Rules の信号 (crate::testing):
    // - 入ったレバー: 弱い信号 15 を全方向、強い信号 15 を真下の読み手にだけ (床に置いたレバーの形)
    // - ダスト: 弱い信号 = power を全方向、強い信号は無し
    const ORIGIN: Pos = Pos::new(0, 0, 0);

    // 弱い信号は隣の読み手へ届く。非導体は中継しない。
    #[test]
    fn weak_signal_reaches_neighbors_only() {
        let mut lv = level();
        let lever = ORIGIN.offset(Up);
        lv.set_state_raw(lever, powered_lever());
        // 読み手がレバーの東にいる: 読み手から源への向きは西。
        assert_eq!(lv.signal(lever, West), 15);
        assert!(lv.has_signal(lever, West));
        assert!(lv.has_neighbor_signal(lever.offset(East)));
        assert_eq!(lv.best_neighbor_signal(lever.offset(East)), 15);
        // 2 マス離れた所へは届かない (間は空気)。
        assert!(!lv.has_neighbor_signal(lever.offset(East).offset(East)));
        // 入っていないレバーは 0。
        lv.set_state_raw(lever, block::LEVER.default_state());
        assert_eq!(lv.signal(lever, West), 0);
        assert!(!lv.has_neighbor_signal(lever.offset(East)));
    }

    // 導体は、受けている強い信号を周りの読み手に見せる。減衰は無い (V05 §2.3)。
    #[test]
    fn conductor_relays_strong_signal() {
        let mut lv = level();
        let stone = ORIGIN;
        let lever = stone.offset(Up);
        lv.set_state_raw(stone, block::STONE.default_state());
        lv.set_state_raw(lever, powered_lever());
        // レバーは真下の石を強充電する。
        assert_eq!(lv.direct_signal(lever, Up), 15);
        assert_eq!(lv.direct_signal_to(stone), 15);
        // 石の横の読み手は、石越しに 15 を読む。
        for dir in Dir::HORIZONTAL {
            assert_eq!(lv.signal(stone, dir.opposite()), 15);
            assert!(lv.has_neighbor_signal(stone.offset(dir)));
        }
        // 石の下の読み手も同じ。
        assert!(lv.has_neighbor_signal(stone.offset(Down)));
    }

    // 強い信号は向きが合う時だけ出る。横のレバーは石を強充電しないので、石は中継しない (V05 §4.3 の裏づけの形)。
    #[test]
    fn strong_signal_is_directional() {
        let mut lv = level();
        let stone = ORIGIN;
        let lever = stone.offset(West);
        lv.set_state_raw(stone, block::STONE.default_state());
        lv.set_state_raw(lever, powered_lever());
        assert_eq!(lv.direct_signal(lever, West), 0);
        assert_eq!(lv.direct_signal_to(stone), 0);
        assert_eq!(lv.signal(stone, West), 0);
        assert!(!lv.has_neighbor_signal(stone.offset(East)));
        // 石そのものは、隣のレバーの弱い信号を受けている。
        assert!(lv.has_neighbor_signal(stone));
    }

    // 中継は 1 段だけ。強充電された導体は、隣の導体を充電しない (V05 §5 の 2)。
    #[test]
    fn relay_is_one_level_deep() {
        let mut lv = level();
        let first = ORIGIN;
        let second = first.offset(East);
        lv.set_state_raw(first, block::STONE.default_state());
        lv.set_state_raw(second, block::STONE.default_state());
        lv.set_state_raw(first.offset(Up), powered_lever());
        assert_eq!(lv.signal(first, West), 15);
        assert_eq!(lv.direct_signal_to(second), 0);
        assert_eq!(lv.signal(second, West), 0);
        assert!(!lv.has_neighbor_signal(second.offset(East)));
    }

    // 導体は弱い信号では充電されない。ダストの隣の石は、その先へ何も見せない (V05 §2.1)。
    #[test]
    fn weak_signal_does_not_charge_conductor() {
        let mut lv = level();
        let stone = ORIGIN;
        lv.set_state_raw(stone, block::STONE.default_state());
        lv.set_state_raw(stone.offset(West), wire_with_power(12));
        assert_eq!(lv.signal(stone.offset(West), West), 12);
        assert_eq!(lv.signal(stone, West), 0);
    }

    // 最大を返す。15 に届かなければ、6 方向を全部見る (V05 §4.1)。
    #[test]
    fn best_neighbor_signal_takes_the_max() {
        let mut lv = level();
        lv.set_state_raw(ORIGIN.offset(West), wire_with_power(4));
        lv.set_state_raw(ORIGIN.offset(South), wire_with_power(9));
        lv.set_state_raw(ORIGIN.offset(Up), wire_with_power(7));
        assert_eq!(lv.best_neighbor_signal(ORIGIN), 9);
        lv.set_state_raw(ORIGIN.offset(Down), powered_lever());
        assert_eq!(lv.best_neighbor_signal(ORIGIN), 15);
    }

    // 高さの外は空気として読む。信号 0・非導体 (V05 §5 の 17)。
    #[test]
    fn out_of_height_reads_as_nothing() {
        let lv = level();
        let top = Pos::new(0, 319, 0);
        assert_eq!(lv.signal(top.offset(Up), Down), 0);
        assert!(!lv.is_conductor(top.offset(Up)));
        assert!(!lv.has_neighbor_signal(top));
    }

    #[test]
    fn conductor_and_source_flags() {
        let mut lv = level();
        lv.set_state_raw(ORIGIN, block::STONE.default_state());
        assert!(lv.is_conductor(ORIGIN));
        assert!(!lv.is_conductor(ORIGIN.offset(Up)));
        assert!(lv.is_signal_source(block::LEVER.default_state()));
        assert!(lv.is_signal_source(block::REDSTONE_WIRE.default_state()));
        assert!(!lv.is_signal_source(block::STONE.default_state()));
        assert!(!lv.is_signal_source(block::REDSTONE_LAMP.default_state()));
    }
}
