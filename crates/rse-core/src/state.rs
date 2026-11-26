//! 状態表: StateId・BlockId・性質のビット・混合基数 (V05・19 §3.1)。
//!
//! - 状態は [`StateId`] (u16) で持つ。エンジンの内部だけの番号で、実機の状態 ID とは合わせない
//!   (版でずれる。V05 §5 の 15)。番号はビルドの時に決まり、世界ごとに振らない。
//! - ブロックの同一性は [`BlockId`] で持つ。予約 tick とブロックイベントの照合 (V02 §4.6・V03 §4.3)、
//!   撤去フックの条件 (V04 §4.2)、近隣更新の送り元 (V04 §5.4) が同一性で決まる。
//! - ブロックの中の状態の添字は、名前順のプロパティの混合基数 (V05 §3.3)。
//!   プロパティの書き換えは桁の差し替えで済む。
//! - 性質は状態ごとの密な表で引く。ビットは別々に持ち、1 つに畳まない (19 §3.1)。
//!
//! 表の中身は [`crate::table`] にある。面の支えと押し反応の列は、使う素子が入る PR で足す。

use crate::table::BLOCKS;
use core::fmt;

/// 性質のビット (状態ごと)。
pub mod flag {
    /// 導体 (V05 §4.4)。強い信号を受けて、周りの読み手に見せる。
    pub const CONDUCTOR: u16 = 1 << 0;
    /// 信号源の印 (V05 §2.1)。ダストが繋がるか・ダイオードの側面入力になるかを決める。
    pub const SIGNAL_SOURCE: u16 = 1 << 1;
    /// 当たり判定がちょうど 1 ブロック (V05 §4.4.1)。
    pub const FULL_COLLISION: u16 = 1 << 2;
    /// BlockEntity を持つ (V04 §4.2 の手順 6・10)。
    pub const HAS_BE: u16 = 1 << 3;
    /// 硬さが -1 (ピストンで動かない。V05 §3.6)。
    pub const UNBREAKABLE: u16 = 1 << 4;
    /// 置き換えられる (V05 §4.7.1)。
    pub const REPLACEABLE: u16 = 1 << 5;
    /// legacy solid (V05 §4.7.2)。
    pub const SOLID: u16 = 1 << 6;
    /// アナログ出力を持つ (V04 §4.1 の手順 8。コンパレーター更新を出す条件)。
    pub const ANALOG_OUTPUT: u16 = 1 << 7;
    /// 空気 (V04 §4.6。形状更新の結果が空気なら壊す)。
    pub const AIR: u16 = 1 << 8;
}

/// 振る舞いの種類。素子の関数は、これを match して振り分ける (19 §2.4・docs/architecture.md §3)。
///
/// 種類は全部ここに席として置く。族の crate が無い種類は、rse-engine が UNIMPLEMENTED を返す。
/// 並びは 19 §2.4 のモジュールの表 (メモの番号) に合わせた。素子の族を足す PR で、足りない種類を足す。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Behavior {
    /// 振る舞いを持たない (材料と飾り)。
    Inert,
    // V06
    Wire,
    // V07
    Repeater,
    Comparator,
    // V08
    Torch,
    WallTorch,
    Observer,
    RedstoneBlock,
    // V09
    Lever,
    Button,
    PressurePlate,
    WeightedPressurePlate,
    TripwireHook,
    Tripwire,
    DaylightDetector,
    Target,
    Lectern,
    LightningRod,
    // V10
    SculkSensor,
    CalibratedSculkSensor,
    // V11
    Lamp,
    CopperBulb,
    NoteBlock,
    Door,
    Trapdoor,
    FenceGate,
    Bell,
    Tnt,
    Jukebox,
    CommandBlock,
    // V12
    Dispenser,
    Dropper,
    Crafter,
    // V13
    Piston,
    StickyPiston,
    PistonHead,
    MovingPiston,
    // V14
    Container,
    TrappedChest,
    Hopper,
    // V15
    Rail,
    PoweredRail,
    DetectorRail,
    ActivatorRail,
    // V16
    Fluid,
    BubbleColumn,
    FallingBlock,
}

impl Behavior {
    /// レール系か。撤去フックを呼ぶ条件が見る (V04 §4.2 の手順 7・§5.7)。
    #[inline]
    pub const fn is_rail(self) -> bool {
        matches!(
            self,
            Behavior::Rail
                | Behavior::PoweredRail
                | Behavior::DetectorRail
                | Behavior::ActivatorRail
        )
    }
}

/// プロパティ 1 つ。値は実機の値の並び (V05 §3.2)。
#[derive(Debug)]
pub struct PropDef {
    pub name: &'static str,
    pub values: &'static [&'static str],
}

/// ブロック 1 種。
#[derive(Debug)]
pub struct BlockDef {
    /// 名前 (名前空間なし)。
    pub name: &'static str,
    /// プロパティ。名前の辞書順 (V05 §3.3 の 1)。
    pub props: &'static [PropDef],
    /// 既定の状態 (プロパティごとの値の添字)。
    pub default: &'static [u8],
    pub behavior: Behavior,
    /// 性質のビット。今の 5 種は状態によらない。状態で割れるブロックは、生成器が状態ごとの表を作る。
    pub flags: u16,
}

/// ブロックの数。
pub const BLOCK_COUNT: usize = BLOCKS.len();

const fn count_states(def: &BlockDef) -> usize {
    let mut n = 1;
    let mut i = 0;
    while i < def.props.len() {
        n *= def.props[i].values.len();
        i += 1;
    }
    n
}

/// ブロックごとの最初の StateId。末尾は状態の総数。
const BASES: [u16; BLOCK_COUNT + 1] = {
    let mut out = [0u16; BLOCK_COUNT + 1];
    let mut i = 0;
    while i < BLOCK_COUNT {
        let next = out[i] as usize + count_states(&BLOCKS[i]);
        assert!(next <= u16::MAX as usize, "状態の総数が u16 を超えた");
        out[i + 1] = next as u16;
        i += 1;
    }
    out
};

/// 状態の総数。
pub const STATE_COUNT: usize = BASES[BLOCK_COUNT] as usize;

/// 状態 → ブロック。
static STATE_BLOCK: [u16; STATE_COUNT] = {
    let mut out = [0u16; STATE_COUNT];
    let mut b = 0;
    while b < BLOCK_COUNT {
        let mut s = BASES[b] as usize;
        while s < BASES[b + 1] as usize {
            out[s] = b as u16;
            s += 1;
        }
        b += 1;
    }
    out
};

/// 状態 → 性質のビット。
static STATE_FLAGS: [u16; STATE_COUNT] = {
    let mut out = [0u16; STATE_COUNT];
    let mut s = 0;
    while s < STATE_COUNT {
        out[s] = BLOCKS[STATE_BLOCK[s] as usize].flags;
        s += 1;
    }
    out
};

/// ブロックの同一性。名前ごとに 1 つ (19 §3.1)。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BlockId(pub u16);

/// ブロックの状態。エンジンの内部だけの番号 (19 §3.1)。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct StateId(pub u16);

/// 縦切りの 5 種の BlockId。
pub mod block {
    use super::BlockId;
    pub const AIR: BlockId = BlockId(0);
    pub const STONE: BlockId = BlockId(1);
    pub const LEVER: BlockId = BlockId(2);
    pub const REDSTONE_WIRE: BlockId = BlockId(3);
    pub const REDSTONE_LAMP: BlockId = BlockId(4);
}

impl BlockId {
    #[inline]
    pub fn def(self) -> &'static BlockDef {
        &BLOCKS[self.0 as usize]
    }

    /// 名前 (名前空間なし) から引く。先頭の `minecraft:` は読み飛ばす。
    pub fn by_name(name: &str) -> Option<BlockId> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        BLOCKS
            .iter()
            .position(|b| b.name == name)
            .map(|i| BlockId(i as u16))
    }

    #[inline]
    pub fn name(self) -> &'static str {
        self.def().name
    }

    #[inline]
    pub fn behavior(self) -> Behavior {
        self.def().behavior
    }

    /// 最初の状態 (全プロパティが最初の値)。
    #[inline]
    pub fn first_state(self) -> StateId {
        StateId(BASES[self.0 as usize])
    }

    #[inline]
    pub fn state_count(self) -> u16 {
        BASES[self.0 as usize + 1] - BASES[self.0 as usize]
    }

    /// 既定の状態 (V05 §3.3 の 4)。
    pub fn default_state(self) -> StateId {
        self.state_from_values(self.def().default)
    }

    /// プロパティの名前から、名前順の添字を引く。
    pub fn prop(self, name: &str) -> Option<usize> {
        self.def().props.iter().position(|p| p.name == name)
    }

    /// 値の添字の列 (名前順) から状態を作る。
    /// 添字は、先頭のプロパティを最上位の桁とする混合基数 (V05 §3.3 の 3)。
    pub fn state_from_values(self, values: &[u8]) -> StateId {
        let props = self.def().props;
        debug_assert_eq!(values.len(), props.len());
        let mut index: u16 = 0;
        for (p, v) in props.iter().zip(values) {
            debug_assert!((*v as usize) < p.values.len());
            index = index * p.values.len() as u16 + *v as u16;
        }
        StateId(BASES[self.0 as usize] + index)
    }
}

impl StateId {
    /// 空気。範囲外と、まだ何も置いていないマスの読みにも使う。
    pub const AIR: StateId = StateId(0);

    #[inline]
    pub fn block(self) -> BlockId {
        BlockId(STATE_BLOCK[self.0 as usize])
    }

    #[inline]
    pub fn flags(self) -> u16 {
        STATE_FLAGS[self.0 as usize]
    }

    #[inline]
    pub fn has(self, flag: u16) -> bool {
        self.flags() & flag != 0
    }

    #[inline]
    pub fn is_air(self) -> bool {
        self.has(flag::AIR)
    }

    /// 導体か (V05 §4.4)。位置で変わる導体 (開いたシュルカーボックス) は、その素子が入る時に関数を通す。
    #[inline]
    pub fn is_conductor(self) -> bool {
        self.has(flag::CONDUCTOR)
    }

    #[inline]
    pub fn behavior(self) -> Behavior {
        self.block().behavior()
    }

    /// ブロックの中の添字。
    #[inline]
    pub fn index_in_block(self) -> u16 {
        self.0 - BASES[STATE_BLOCK[self.0 as usize] as usize]
    }

    /// プロパティ `prop` (名前順の添字) の桁の重みと、値の個数。
    fn digit(self, prop: usize) -> (u16, u16) {
        let props = self.block().def().props;
        let weight: u16 = props[prop + 1..]
            .iter()
            .map(|p| p.values.len() as u16)
            .product();
        (weight, props[prop].values.len() as u16)
    }

    /// プロパティの値の添字を読む。
    pub fn value(self, prop: usize) -> u8 {
        let (weight, count) = self.digit(prop);
        (self.index_in_block() / weight % count) as u8
    }

    /// プロパティの値を差し替えた状態。桁の差し替えで求める (V05 §7.1)。
    pub fn with_value(self, prop: usize, value: u8) -> StateId {
        let (weight, count) = self.digit(prop);
        debug_assert!((value as u16) < count);
        let current = self.index_in_block() / weight % count;
        StateId(self.0 - current * weight + value as u16 * weight)
    }

    /// 値の並びで次の値へ進めた状態。末尾なら先頭へ戻る (V05 §3.2。StateHolder.cycle)。
    pub fn cycle(self, prop: usize) -> StateId {
        let (_, count) = self.digit(prop);
        self.with_value(prop, ((self.value(prop) as u16 + 1) % count) as u8)
    }

    /// プロパティの値の名前を読む。
    pub fn value_name(self, prop: usize) -> &'static str {
        self.block().def().props[prop].values[self.value(prop) as usize]
    }

    /// blockstate の文字列から引く。形は `名前` か `名前[k=v,k=v]`。
    /// 先頭の `minecraft:` は読み飛ばす。書いていないプロパティは既定の値になる。
    pub fn parse(text: &str) -> Result<StateId, ParseError> {
        let (name, rest) = match text.find('[') {
            Some(at) => (&text[..at], Some(&text[at + 1..])),
            None => (text, None),
        };
        let block = BlockId::by_name(name).ok_or(ParseError::UnknownBlock)?;
        let mut state = block.default_state();
        let Some(rest) = rest else {
            return Ok(state);
        };
        let body = rest.strip_suffix(']').ok_or(ParseError::Syntax)?;
        if body.is_empty() {
            return Ok(state);
        }
        for pair in body.split(',') {
            let (key, value) = pair.split_once('=').ok_or(ParseError::Syntax)?;
            let prop = block.prop(key.trim()).ok_or(ParseError::UnknownProperty)?;
            let value = value.trim();
            let at = block.def().props[prop]
                .values
                .iter()
                .position(|v| *v == value)
                .ok_or(ParseError::UnknownValue)?;
            state = state.with_value(prop, at as u8);
        }
        Ok(state)
    }
}

/// blockstate の文字列。プロパティは名前順で、全部書く (`redstone_lamp[lit=false]`)。
impl fmt::Display for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let def = self.block().def();
        f.write_str(def.name)?;
        for (i, p) in def.props.iter().enumerate() {
            f.write_str(if i == 0 { "[" } else { "," })?;
            write!(f, "{}={}", p.name, self.value_name(i))?;
        }
        if !def.props.is_empty() {
            f.write_str("]")?;
        }
        Ok(())
    }
}

/// blockstate の文字列を引けなかった理由。未知の名前は断る (19 §3.1 の「未知」の段)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseError {
    /// 表に無いブロックの名前。
    UnknownBlock,
    /// そのブロックに無いプロパティ。
    UnknownProperty,
    /// そのプロパティに無い値。
    UnknownValue,
    /// 形が崩れている。
    Syntax,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn five_blocks_in_fixed_order() {
        let names: Vec<&str> = BLOCKS.iter().map(|b| b.name).collect();
        assert_eq!(
            names,
            ["air", "stone", "lever", "redstone_wire", "redstone_lamp"]
        );
        assert_eq!(block::AIR.name(), "air");
        assert_eq!(block::STONE.name(), "stone");
        assert_eq!(block::LEVER.name(), "lever");
        assert_eq!(block::REDSTONE_WIRE.name(), "redstone_wire");
        assert_eq!(block::REDSTONE_LAMP.name(), "redstone_lamp");
        assert_eq!(StateId::AIR, block::AIR.default_state());
    }

    // 状態の数は V05 §3.4 の表のとおり。
    #[test]
    fn state_counts_match_notes() {
        assert_eq!(block::AIR.state_count(), 1);
        assert_eq!(block::STONE.state_count(), 1);
        assert_eq!(block::LEVER.state_count(), 24);
        assert_eq!(block::REDSTONE_WIRE.state_count(), 1296);
        assert_eq!(block::REDSTONE_LAMP.state_count(), 2);
        assert_eq!(STATE_COUNT, 1 + 1 + 24 + 1296 + 2);
    }

    // 表の書き方の決まり: プロパティは名前の辞書順で、既定の値は範囲の内 (V05 §3.3 の 1)。
    #[test]
    fn table_is_well_formed() {
        for def in &BLOCKS {
            assert!(
                def.props.windows(2).all(|w| w[0].name < w[1].name),
                "{} のプロパティが名前順でない",
                def.name
            );
            assert_eq!(def.default.len(), def.props.len(), "{}", def.name);
            for (p, v) in def.props.iter().zip(def.default) {
                assert!((*v as usize) < p.values.len(), "{}.{}", def.name, p.name);
            }
        }
    }

    // V05 §3.3 の例: ダストの既定 (四方 none・power 0) の添字は
    // 2×432 + 2×144 + 0×9 + 2×3 + 2 = 1160。
    #[test]
    fn mixed_radix_matches_wire_example() {
        let wire = block::REDSTONE_WIRE;
        assert_eq!(wire.default_state().index_in_block(), 1160);
        // 名前が最後のプロパティが一番速く回る (V05 §3.3 の 2)。
        let first = wire.first_state();
        assert_eq!(
            first.to_string(),
            "redstone_wire[east=up,north=up,power=0,south=up,west=up]"
        );
        assert_eq!(
            StateId(first.0 + 1).to_string(),
            "redstone_wire[east=up,north=up,power=0,south=up,west=side]"
        );
        assert_eq!(
            StateId(first.0 + 3).to_string(),
            "redstone_wire[east=up,north=up,power=0,south=side,west=up]"
        );
        assert_eq!(
            StateId(first.0 + 9).to_string(),
            "redstone_wire[east=up,north=up,power=1,south=up,west=up]"
        );
        assert_eq!(
            StateId(first.0 + 144).to_string(),
            "redstone_wire[east=up,north=side,power=0,south=up,west=up]"
        );
        assert_eq!(
            StateId(first.0 + 432).to_string(),
            "redstone_wire[east=side,north=up,power=0,south=up,west=up]"
        );
    }

    // 既定の状態 (V05 §3.4): レバーは wall・north・false、ランプは lit=false。
    #[test]
    fn defaults_match_notes() {
        assert_eq!(
            block::LEVER.default_state().to_string(),
            "lever[face=wall,facing=north,powered=false]"
        );
        assert_eq!(block::LEVER.default_state().index_in_block(), 9);
        assert_eq!(
            block::REDSTONE_LAMP.default_state().to_string(),
            "redstone_lamp[lit=false]"
        );
        assert_eq!(
            block::REDSTONE_WIRE.default_state().to_string(),
            "redstone_wire[east=none,north=none,power=0,south=none,west=none]"
        );
        assert_eq!(StateId::AIR.to_string(), "air");
    }

    // 全状態で、値を読む → 同じ値で作り直す、が元に戻る。差し替えはその桁だけを変える。
    #[test]
    fn digits_round_trip_for_every_state() {
        for s in 0..STATE_COUNT as u16 {
            let state = StateId(s);
            let block = state.block();
            let props = block.def().props;
            let values: Vec<u8> = (0..props.len()).map(|i| state.value(i)).collect();
            assert_eq!(block.state_from_values(&values), state);
            for (i, p) in props.iter().enumerate() {
                for v in 0..p.values.len() as u8 {
                    let changed = state.with_value(i, v);
                    assert_eq!(changed.block(), block);
                    for (j, before) in values.iter().enumerate() {
                        let expect = if i == j { v } else { *before };
                        assert_eq!(changed.value(j), expect);
                    }
                }
            }
        }
    }

    // 全状態で、文字列にして読み直すと元に戻る。
    #[test]
    fn display_and_parse_round_trip() {
        for s in 0..STATE_COUNT as u16 {
            let state = StateId(s);
            assert_eq!(StateId::parse(&state.to_string()), Ok(state));
        }
    }

    #[test]
    fn parse_accepts_namespace_and_partial_props() {
        let lamp = block::REDSTONE_LAMP;
        let lit = lamp.prop("lit").unwrap();
        assert_eq!(
            StateId::parse("minecraft:redstone_lamp[lit=true]"),
            Ok(lamp.default_state().with_value(lit, 0))
        );
        assert_eq!(StateId::parse("redstone_lamp"), Ok(lamp.default_state()));
        assert_eq!(StateId::parse("redstone_lamp[]"), Ok(lamp.default_state()));
        // 書いていないプロパティは既定の値。
        let wire = StateId::parse("redstone_wire[power=15]").unwrap();
        assert_eq!(
            wire.to_string(),
            "redstone_wire[east=none,north=none,power=15,south=none,west=none]"
        );
    }

    #[test]
    fn parse_rejects_unknown() {
        assert_eq!(StateId::parse("repeater"), Err(ParseError::UnknownBlock));
        assert_eq!(
            StateId::parse("redstone_lamp[power=1]"),
            Err(ParseError::UnknownProperty)
        );
        assert_eq!(
            StateId::parse("redstone_lamp[lit=maybe]"),
            Err(ParseError::UnknownValue)
        );
        assert_eq!(
            StateId::parse("redstone_lamp[lit=true"),
            Err(ParseError::Syntax)
        );
        assert_eq!(
            StateId::parse("redstone_lamp[lit]"),
            Err(ParseError::Syntax)
        );
    }

    // cycle は値の並びで次へ進み、末尾で先頭へ戻る (V05 §3.2)。真偽は true → false → true。
    #[test]
    fn cycle_follows_value_order() {
        let lever = block::LEVER.default_state();
        let powered = block::LEVER.prop("powered").unwrap();
        assert_eq!(lever.value_name(powered), "false");
        assert_eq!(lever.cycle(powered).value_name(powered), "true");
        assert_eq!(lever.cycle(powered).cycle(powered), lever);
        let wire = block::REDSTONE_WIRE.default_state();
        let power = block::REDSTONE_WIRE.prop("power").unwrap();
        assert_eq!(wire.with_value(power, 15).cycle(power), wire);
    }

    // 性質のビット (V05 §4.3・§4.4・§4.7)。
    #[test]
    fn flags_of_five_blocks() {
        let air = StateId::AIR;
        let stone = block::STONE.default_state();
        let lever = block::LEVER.default_state();
        let wire = block::REDSTONE_WIRE.default_state();
        let lamp = block::REDSTONE_LAMP.default_state();
        assert!(air.is_air() && air.has(flag::REPLACEABLE) && !air.is_conductor());
        assert!(stone.is_conductor() && stone.has(flag::SOLID) && !stone.has(flag::SIGNAL_SOURCE));
        assert!(!lever.is_conductor() && lever.has(flag::SIGNAL_SOURCE) && !lever.has(flag::SOLID));
        assert!(!wire.is_conductor() && wire.has(flag::SIGNAL_SOURCE) && !wire.has(flag::SOLID));
        assert!(lamp.is_conductor() && !lamp.has(flag::SIGNAL_SOURCE));
        for s in [air, stone, lever, wire, lamp] {
            assert!(!s.has(flag::HAS_BE) && !s.has(flag::ANALOG_OUTPUT));
        }
        assert_eq!(lever.behavior(), Behavior::Lever);
        assert_eq!(wire.behavior(), Behavior::Wire);
        assert_eq!(lamp.behavior(), Behavior::Lamp);
        assert_eq!(stone.behavior(), Behavior::Inert);
    }

    #[test]
    fn rail_kinds() {
        assert!(Behavior::Rail.is_rail() && Behavior::DetectorRail.is_rail());
        assert!(!Behavior::Wire.is_rail() && !Behavior::Inert.is_rail());
    }
}
