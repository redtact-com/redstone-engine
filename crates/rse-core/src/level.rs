//! 世界 1 つ分の状態 (docs/architecture.md §3)。
//!
//! 手順は、メモの分け方に合わせて別のモジュールに置く
//! (書き込みは [`crate::set_block`]、更新は [`crate::update`]、予約 tick は [`crate::ticks`] ほか)。

use crate::events::BlockEvents;
use crate::rules::Rules;
use crate::state::{BlockId, StateId};
use crate::status::Status;
use crate::ticks::LevelTicks;
use crate::update::Updater;
use crate::world::{Change, World};
use rse_java::Pos;

/// 世界の設定。
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// 書ける高さの下端。既定の -64 は overworld の最小 Y (V04 §4.1 の「手順 1 の範囲の値」)。
    /// 書けるのは -64 以上 319 以下。水平の範囲 (チャンク座標の絶対値 2,097,061 以下) は、格子の枠の方が狭い。
    pub min_y: i32,
    /// 書ける高さの幅。既定は 384 (V04 §4.7 の「ワールドの高さ 384」)。
    pub height: i32,
    /// 近隣更新の連鎖の上限。既定 1,000,000。負は無制限 (V04 §3.2・V18 §4.9)。
    pub max_chained_updates: i32,
    /// 1 tick に集める予約 tick の上限。既定 65,536 (V02 §3.5)。
    pub max_ticks_per_tick: usize,
    /// ブロックイベントの安全弁。1 tick に配る件数の上限で、`None` は無し (実機と同じ)。
    /// 働いたら SAFETY を返す (V03 §7.2)。
    pub block_event_safety: Option<u32>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            min_y: -64,
            height: 384,
            max_chained_updates: 1_000_000,
            max_ticks_per_tick: 65_536,
            block_event_safety: None,
        }
    }
}

/// 世界 1 つ。素子の振る舞いは `R` で静的に決まる。
pub struct Level<R: Rules> {
    /// 素子が世界ごとに持つ状態 (例: ダストの「評価中」の印。V05 §7.2)。骨格は中身を見ない。
    pub rules: R,
    pub(crate) world: World,
    pub(crate) updater: Updater,
    pub(crate) block_ticks: LevelTicks<BlockId>,
    pub(crate) events: BlockEvents,
    pub(crate) config: Config,
    /// ゲーム時刻。予約 tick の基準 (V01 §2.3 の L5)。
    pub(crate) game_time: i64,
    /// 時計 (V01 §2.2 の B2)。縦切りでは値だけを進める。
    pub(crate) clock: i64,
    /// 予約順のカウンター。ブロック用と流体用で共有する (V02 §3.6)。
    pub(crate) sub_tick: i64,
    /// tick の先頭からブロックイベントの相の終わりまで真 (V03 §7.3)。
    pub(crate) handling_tick: bool,
    /// 空の暗さ。外から与えた値を L4 で読む (19 §4.1)。
    pub(crate) sky_darken: i32,
    pub(crate) sky_darken_input: i32,
    /// 前に取り出した後に立った状態コード。
    pub(crate) status: Status,
}

impl<R: Rules> Level<R> {
    pub fn new(rules: R, config: Config) -> Level<R> {
        Level {
            rules,
            world: World::new(config.min_y, config.height),
            updater: Updater::new(config.max_chained_updates),
            block_ticks: LevelTicks::new(),
            events: BlockEvents::new(),
            config,
            game_time: 0,
            clock: 0,
            sub_tick: 0,
            handling_tick: false,
            sky_darken: 0,
            sky_darken_input: 0,
            status: Status::OK,
        }
    }

    /// 今の状態を読む。高さの外は空気 (V05 §2.3 の 1)。
    #[inline]
    pub fn get_state(&self, pos: Pos) -> StateId {
        self.world.get(pos)
    }

    /// 更新を出さずに置く (19 §4.2 の「そのまま」の型)。フックも呼ばない。
    /// 書けなかったら偽 (高さの外・格子の枠の外)。
    pub fn set_state_raw(&mut self, pos: Pos, state: StateId) -> bool {
        self.world.set(pos, state).is_some()
    }

    /// 書ける高さの内か。
    #[inline]
    pub fn in_bounds(&self, pos: Pos) -> bool {
        self.world.in_bounds(pos)
    }

    #[inline]
    pub fn game_time(&self) -> i64 {
        self.game_time
    }

    #[inline]
    pub fn clock(&self) -> i64 {
        self.clock
    }

    /// tick の先頭からブロックイベントの相の終わりまで真 (V01 §2.3 の L0・L11)。ピストンが読む。
    #[inline]
    pub fn handling_tick(&self) -> bool {
        self.handling_tick
    }

    /// 空の暗さ (L4 で読み込んだ値)。
    #[inline]
    pub fn sky_darken(&self) -> i32 {
        self.sky_darken
    }

    /// 空の暗さを外から与える。次の L4 で反映する (19 §4.1)。
    pub fn set_sky_darken(&mut self, value: i32) {
        self.sky_darken_input = value;
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 状態コードを立てる。素子が未実装の経路を通った時などに使う。
    #[inline]
    pub fn raise(&mut self, status: Status) {
        self.status |= status;
    }

    /// 立っている状態コードを取り出して消す。
    pub fn take_status(&mut self) -> Status {
        core::mem::take(&mut self.status)
    }

    /// 前の書き出しの後の、正味の変化を返す (19 §3.8)。
    pub fn drain_changes(&mut self) -> &[Change] {
        self.world.drain_changes()
    }
}
