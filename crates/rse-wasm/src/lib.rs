//! wasm の窓口 (C ABI。19 §2.5)。表と決まりは docs/wasm-abi.md にある。
//!
//! JS とは、素の C ABI の関数と線形メモリだけでやり取りする。wasm-bindgen を使わず、
//! wasm の import は 0 個にする。Node・ブラウザで同じに動く。
//!
//! | 関数 | 役 |
//! |---|---|
//! | `rse_abi_version` | 窓口の版 ([`ABI_VERSION`]) を返す (前からある) |
//! | `rse_scratch` | 作業領域を広げ、その先頭の番地を返す (7 関数 + 1 の「+1」) |
//! | `rse_world_new`・`rse_world_free` | 世界を作る・捨てる。世界はハンドル (番号) で持つ |
//! | `rse_resolve` | blockstate の名前を StateId に解決する。文字列が境界を越えるのはここだけ |
//! | `rse_load` | マスの記録の列 ([`CellRec`]) を世界に置く |
//! | `rse_step` | n tick 進める |
//! | `rse_input` | tick の外の入力を 1 件入れる (区間の印つき。19 §4.1) |
//! | `rse_changes` | 前に取り出した後の正味の変化を、マスの記録の列で返す (19 §3.8) |
//!
//! 決まり:
//! - JS が書いてよい所は作業領域だけにする。ポインタの引数は、作業領域の内でなければ
//!   [`status::ARGUMENT`] を返し、何も読み書きしない。JS の誤りでエンジンの値を壊さない。
//! - エラーは状態コード (ビットの集合) で返し、黙って捨てない。下の 16 ビットはエンジンの状態コード
//!   (rse-core の `Status`)、上の 16 ビットは窓口の状態コード ([`status`])。文は JS 側の表で引く。
//! - unsafe のブロックは scratch.rs だけに置く (作業領域の読み書き)。exports.rs は
//!   `#[unsafe(no_mangle)]` のために unsafe_code を許すが、unsafe のブロックは書かない
//!   (tools/ip-hygiene.test.ts が見張る)。
//! - std のまま組む。割り当て器 (std の dlmalloc) と、世界の表を持つ thread_local を std から借りる。
//!   no_std にすると、割り当て器を自分で書く (unsafe が増える) か外部の crate を足すことになる。
#![deny(unsafe_code)]
#![deny(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block
)]

mod exports;
mod registry;
mod scratch;

#[cfg(test)]
mod tests;

pub use exports::*;

/// 窓口の版。JS 側は読み込んだ wasm がこの版と合うかを確かめる。
/// 関数の形・記録の並び・状態コードのビットを変えたら上げる。
pub const ABI_VERSION: u32 = 1;

/// マスの記録 (16 バイト)。読み込みの列 (`rse_load`) と変化の列 (`rse_changes`) で同じ形を使う。
///
/// 4 つとも 32 ビットの整数で、詰め物は無い。JS は `Int32Array` の窓で 4 つずつ読む。
/// `state` は StateId (u16) を 32 ビットに広げた物。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CellRec {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub state: u32,
}

const _: () = assert!(core::mem::size_of::<CellRec>() == 16);
const _: () = assert!(core::mem::align_of::<CellRec>() == 4);

/// 窓口の状態コード (上の 16 ビット)。エンジンの状態コード (下の 16 ビット) とは重ならない。
pub mod status {
    /// 世界のハンドルが無い (作っていない・もう捨てた)。
    pub const HANDLE: u32 = 1 << 16;
    /// 引数が決まりの外 (ポインタが作業領域の外・揃っていない・種類や区間の番号が無い・StateId が表の外)。
    pub const ARGUMENT: u32 = 1 << 17;
    /// 置けない (高さの外・格子の枠の外)。
    pub const OUT_OF_WORLD: u32 = 1 << 18;
    /// 状態表に無いブロックの名前 (19 §11 の 5)。
    pub const UNKNOWN_BLOCK: u32 = 1 << 19;
    /// そのブロックに無いプロパティ。
    pub const UNKNOWN_PROPERTY: u32 = 1 << 20;
    /// そのプロパティに無い値。
    pub const UNKNOWN_VALUE: u32 = 1 << 21;
    /// 名前の形が崩れている (UTF-8 でない時も含む)。
    pub const SYNTAX: u32 = 1 << 22;

    /// 一覧 (ビットの番号の順)。JS 側の表と突き合わせるために使う。
    pub const ALL: [(u32, &str); 7] = [
        (HANDLE, "HANDLE"),
        (ARGUMENT, "ARGUMENT"),
        (OUT_OF_WORLD, "OUT_OF_WORLD"),
        (UNKNOWN_BLOCK, "UNKNOWN_BLOCK"),
        (UNKNOWN_PROPERTY, "UNKNOWN_PROPERTY"),
        (UNKNOWN_VALUE, "UNKNOWN_VALUE"),
        (SYNTAX, "SYNTAX"),
    ];
}

/// 読み込みの型 (`rse_load` の `mode`。19 §4.2)。
pub mod load_mode {
    /// そのまま: 更新を出さずに置く。fixture が使う。
    pub const SETTLED: u32 = 0;
    /// 組み直し: 置いてから全ブロックへ NC を配り、静かになるまで進める。席だけで、UNIMPLEMENTED を返す。
    pub const REBUILD: u32 = 1;
}

/// 入力の種類 (`rse_input` の `kind`)。
pub mod input_kind {
    /// 使う (右クリック)。`arg` は 0。
    pub const USE: u32 = 0;
    /// setblock。`arg` の下の 16 ビットが StateId、ビット 16 が strict。席だけで、UNIMPLEMENTED を返す。
    pub const SET_BLOCK: u32 = 1;
}
