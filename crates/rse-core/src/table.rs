//! 手で書いた状態表 (縦切りの 5 種)。
//!
//! 後の PR で、生成器 (19 §3.1 の「生成の手順」) の出力に置き換える。表の形 ([`BlockDef`]) は変えない。
//!
//! 書き方の決まり (V05 §3.2・§3.3):
//! - プロパティは**名前の辞書順**に並べる。状態の添字は、この順の混合基数で決まる。
//! - 値は実機の値の並びで書く。真偽は true → false、整数は昇順、face は floor → wall → ceiling、
//!   水平の向きは north → south → west → east (水平の反復の順 北・東・南・西 とは違う)。
//! - 既定の状態は、値の添字の列で書く (V05 §3.4 の表)。

use crate::state::{Behavior, BlockDef, PropDef, flag};

const BOOL: &[&str] = &["true", "false"];
/// ダストの接続 (V06 §3.1)。
const WIRE_SIDE: &[&str] = &["up", "side", "none"];
const POWER: &[&str] = &[
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
];
const ATTACH_FACE: &[&str] = &["floor", "wall", "ceiling"];
const HORIZONTAL_FACING: &[&str] = &["north", "south", "west", "east"];

/// 当たり判定がちょうど 1 ブロックの、ふつうの立方体。既定の導体の判定で導体になる (V05 §4.4.1)。
const FULL_CUBE: u16 = flag::CONDUCTOR | flag::FULL_COLLISION | flag::SOLID;

/// ブロックの一覧。添字が BlockId になる。番号はエンジンの内部だけの物で、実機の ID とは合わせない (19 §3.1)。
pub const BLOCKS: [BlockDef; 5] = [
    // 空気。置き換えられる (V05 §4.7.1)。
    BlockDef {
        name: "air",
        props: &[],
        default: &[],
        behavior: Behavior::Inert,
        flags: flag::AIR | flag::REPLACEABLE,
    },
    // 石。導体の代表。
    BlockDef {
        name: "stone",
        props: &[],
        default: &[],
        behavior: Behavior::Inert,
        flags: FULL_CUBE,
    },
    // レバー。24 状態。既定は wall・north・false (V05 §3.4)。
    // 信号源 (V05 §4.3)。当たり判定が無いので非導体で、legacy solid も偽 (V05 §4.7.2)。
    BlockDef {
        name: "lever",
        props: &[
            PropDef {
                name: "face",
                values: ATTACH_FACE,
            },
            PropDef {
                name: "facing",
                values: HORIZONTAL_FACING,
            },
            PropDef {
                name: "powered",
                values: BOOL,
            },
        ],
        default: &[1, 0, 1],
        behavior: Behavior::Lever,
        flags: flag::SIGNAL_SOURCE,
    },
    // ダスト。1296 状態。既定は四方 none・power 0 (V05 §3.3 の例・V06 §3.1)。
    // 信号源の印は表では真。評価中だけ偽になる分は Rules.is_signal_source が受け持つ (V05 §7.2)。
    BlockDef {
        name: "redstone_wire",
        props: &[
            PropDef {
                name: "east",
                values: WIRE_SIDE,
            },
            PropDef {
                name: "north",
                values: WIRE_SIDE,
            },
            PropDef {
                name: "power",
                values: POWER,
            },
            PropDef {
                name: "south",
                values: WIRE_SIDE,
            },
            PropDef {
                name: "west",
                values: WIRE_SIDE,
            },
        ],
        default: &[2, 2, 0, 2, 2],
        behavior: Behavior::Wire,
        flags: flag::SIGNAL_SOURCE,
    },
    // ランプ。2 状態。既定は lit=false (V05 §3.4)。導体の上書きの一覧 (V05 §4.4.2) に無いので導体。
    BlockDef {
        name: "redstone_lamp",
        props: &[PropDef {
            name: "lit",
            values: BOOL,
        }],
        default: &[1],
        behavior: Behavior::Lamp,
        flags: FULL_CUBE,
    },
];
