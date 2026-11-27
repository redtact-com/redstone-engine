//! 書き出す関数 (C ABI)。中身は registry.rs にあり、ここは引数を渡すだけ。
//!
//! JS から来たポインタは、番地の数 (`addr`) にしてから渡す。読み書きは scratch.rs が、
//! 作業領域の内かを確かめてから行う。
//!
//! `#[unsafe(no_mangle)]` は、名前が wasm の書き出しにそのまま出る属性で、同じ名前の記号が 2 つあると
//! 壊れる。名前は全部 `rse_` で始め、この crate のほかの所と std に同じ名前は無い。
//! この属性のためだけに unsafe_code を許す。このファイルに unsafe のブロックは書かない
//! (tools/ip-hygiene.test.ts が見張る)。
#![allow(unsafe_code)]

use crate::registry::with;
use crate::{ABI_VERSION, CellRec};

/// 窓口の版を返す。
#[unsafe(no_mangle)]
pub extern "C" fn rse_abi_version() -> u32 {
    ABI_VERSION
}

/// 作業領域を `len` バイト以上に広げ (縮めない)、先頭の番地を返す。割り当てに失敗したら null。
///
/// 番地は 8 バイトに揃う。広げると前に返した番地は使えなくなり、線形メモリが伸びることもある。
/// JS は、この関数を呼んだ後に番地と型付き配列の窓を取り直してから書く。
#[unsafe(no_mangle)]
pub extern "C" fn rse_scratch(len: u32) -> *mut u8 {
    with(|r| r.scratch(len))
}

/// 世界を作り、ハンドルを返す。席が尽きたら 0 (限りは registry.rs の冒頭)。
#[unsafe(no_mangle)]
pub extern "C" fn rse_world_new() -> u32 {
    with(|r| r.world_new())
}

/// 世界を捨てる。ハンドルが無ければ HANDLE を返す (2 回捨てた時も)。
///
/// 19 §2.5 の例は返り値を持たないが、古いハンドルや 2 回目の解放を黙って通さないために状態コードを返す。
#[unsafe(no_mangle)]
pub extern "C" fn rse_world_free(handle: u32) -> u32 {
    with(|r| r.world_free(handle))
}

/// `ptr` から `len` バイトの UTF-8 の blockstate の名前を StateId に解決し、`out` に u32 で書く。
///
/// `ptr..ptr + len` と `out..out + 4` は作業領域の内で、`out` は 4 バイトに揃うこと (外なら ARGUMENT)。
/// 名前を解決できなければ UNKNOWN_BLOCK・UNKNOWN_PROPERTY・UNKNOWN_VALUE・SYNTAX のどれかを返し、
/// `out` に書かない。名前の形は `StateId::parse` のとおり (先頭の `minecraft:` は読み飛ばし、
/// 書いていないプロパティは既定の値)。世界に依らないので、ハンドルを取らない。
#[unsafe(no_mangle)]
pub extern "C" fn rse_resolve(ptr: *const u8, len: u32, out: *mut u32) -> u32 {
    with(|r| r.resolve(ptr.addr(), len, out.addr()))
}

/// `recs` から `n` 件のマスの記録を、世界に置く。
///
/// `recs..recs + 16n` は作業領域の内で、4 バイトに揃うこと (外なら ARGUMENT)。
/// `mode` は `load_mode`。SETTLED は更新を出さずに置く (19 §4.2 の「そのまま」)。REBUILD は席だけで
/// UNIMPLEMENTED を返す。StateId が表の外 (ARGUMENT) か、高さの外・格子の枠の外 (OUT_OF_WORLD) のマスが
/// あれば、1 つも置かない (格子の枠は置いてみて分かるので、置いた分を戻す)。置いたマスは変化の列に載る。
#[unsafe(no_mangle)]
pub extern "C" fn rse_load(handle: u32, recs: *const CellRec, n: u32, mode: u32) -> u32 {
    with(|r| r.load(handle, recs.addr(), n, mode))
}

/// `n` tick 進め、その間に立ったエンジンの状態コードを返す (0 なら何も無い)。
#[unsafe(no_mangle)]
pub extern "C" fn rse_step(handle: u32, n: u32) -> u32 {
    with(|r| r.step(handle, n))
}

/// tick の外の入力を 1 件入れ、連鎖を回し切ってから、立ったエンジンの状態コードを返す。
///
/// `kind` は `input_kind`、`section` は区間の印 (0 = B5・1 = C・2 = A・3 = B1。19 §4.1)。
/// 区間の印は順を変えない (呼んだ順に適用する)。`arg` は、use なら 0、setblock なら下の 16 ビットが StateId で
/// ビット 16 が strict (ビット 17 以上が立っていれば ARGUMENT)。
#[unsafe(no_mangle)]
pub extern "C" fn rse_input(
    handle: u32,
    kind: u32,
    section: u32,
    x: i32,
    y: i32,
    z: i32,
    arg: u32,
) -> u32 {
    with(|r| r.input(handle, kind, section, x, y, z, arg))
}

/// 前に取り出した後の正味の変化を取り出し、マスの記録の列の先頭を返す。件数は `len_out` に u32 で書く。
///
/// `len_out` は作業領域の内で 4 バイトに揃うこと。外なら null を返し、変化を取り出さない。
/// ハンドルが無ければ null を返し、`len_out` に 0 を書く。列は、次にこの世界の `rse_changes` を
/// 呼ぶか、世界を捨てるまで動かない。ただし線形メモリが伸びると、JS の窓は作り直しになる。
#[unsafe(no_mangle)]
pub extern "C" fn rse_changes(handle: u32, len_out: *mut u32) -> *const CellRec {
    with(|r| r.changes(handle, len_out.addr()))
}
