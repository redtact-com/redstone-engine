//! 作業領域: JS が名前と読み込みの列を書き、窓口が書き出しの値 (StateId・件数) を返す所。
//!
//! **unsafe のブロックはこのファイルだけに置く**。どれも線形メモリ (作業領域) の読み書きに閉じる。
//!
//! - 中身は `Vec<u64>` で持つ。先頭は 8 バイトに揃い、0 で埋めてから渡すので、未初期化の所を読まない。
//! - JS から来た番地は数としてだけ使い、作業領域の内かを確かめてから、作業領域の Vec 自身のポインタに
//!   位置を足して読み書きする。番地が作業領域の外なら、何も読み書きしない。
//! - 作業領域を広げる (中身を移す) のは [`Scratch::ensure`] (`rse_scratch`) だけ。ほかの関数は広げない。
//!   広げると前に返した番地は使えなくなるので、JS は `rse_scratch` を呼ぶたびに番地を取り直す。
#![allow(unsafe_code)]

use crate::CellRec;
use core::mem::{align_of, size_of};
use core::slice;

pub(crate) struct Scratch {
    words: Vec<u64>,
}

impl Scratch {
    pub(crate) const fn new() -> Scratch {
        Scratch { words: Vec::new() }
    }

    pub(crate) fn len_bytes(&self) -> usize {
        self.words.len() * size_of::<u64>()
    }

    /// `len` バイト以上に広げ (縮めない)、先頭の番地を返す。割り当てに失敗したら `None`。
    /// 広げた分は 0 で埋める。前からある分の中身は保つ。
    pub(crate) fn ensure(&mut self, len: usize) -> Option<*mut u8> {
        let words = len.div_ceil(size_of::<u64>());
        if words > self.words.len() {
            // 割り当ての失敗で abort しないよう、先に確保を試す。
            self.words.try_reserve(words - self.words.len()).ok()?;
            self.words.resize(words, 0);
        }
        Some(self.words.as_mut_ptr().cast::<u8>())
    }

    /// `addr..addr + len` が作業領域の内で、`addr` が `align` の倍数なら、先頭からの位置を返す。
    fn offset(&self, addr: usize, len: usize, align: usize) -> Option<usize> {
        let base = self.words.as_ptr().addr();
        let at = addr.checked_sub(base)?;
        let end = at.checked_add(len)?;
        // 先頭は 8 バイトに揃うので、位置が align の倍数なら番地も align の倍数になる。
        (end <= self.len_bytes() && at % align == 0).then_some(at)
    }

    /// `addr` から `len` バイトを読む。作業領域の外なら `None`。
    pub(crate) fn bytes(&self, addr: usize, len: usize) -> Option<&[u8]> {
        let at = self.offset(addr, len, 1)?;
        let start = self.words.as_ptr().cast::<u8>().wrapping_add(at);
        // SAFETY: `offset` が `at..at + len` を作業領域の確保 (`len_bytes` バイト) の内と確かめたので、
        // `start` から `len` バイトは 1 つの確保の内にあり、読める。中身は `ensure` が 0 で埋めた後に
        // JS が書いたバイトで、u8 はどのビットの並びも正しい値である。返す参照は `&self` に縛られ、
        // その間に作業領域を書き換える `&mut self` の関数は呼べない。JS は wasm の中が動いている間は書かない
        // (単一スレッドで、import が無いので呼び返しも無い)。`len` は確保の大きさ以下なので isize に収まる。
        Some(unsafe { slice::from_raw_parts(start, len) })
    }

    /// `addr` から `n` 件のマスの記録を読む。作業領域の外か、4 バイトに揃っていなければ `None`。
    pub(crate) fn cells(&self, addr: usize, n: usize) -> Option<&[CellRec]> {
        let len = n.checked_mul(size_of::<CellRec>())?;
        let at = self.offset(addr, len, align_of::<CellRec>())?;
        let start = self
            .words
            .as_ptr()
            .cast::<u8>()
            .wrapping_add(at)
            .cast::<CellRec>();
        // SAFETY: `offset` が `at..at + len` を作業領域の確保の内と、`start` が CellRec の揃え (4) の倍数で
        // あることを確かめた。CellRec は `#[repr(C)]` の 32 ビットの整数 4 つで詰め物が無く、どのビットの
        // 並びも正しい値なので、0 で埋めた後に JS が書いたバイトをそのまま読める。参照の寿命と書き換えの
        // 排他は `bytes` と同じ。`len` は確保の大きさ以下なので isize に収まる。
        Some(unsafe { slice::from_raw_parts(start, n) })
    }

    /// `addr` に u32 を書けるか (作業領域の内で、4 バイトに揃っている)。
    pub(crate) fn fits_u32(&self, addr: usize) -> bool {
        self.offset(addr, size_of::<u32>(), align_of::<u32>())
            .is_some()
    }

    /// `addr` に u32 を書く。作業領域の外か、4 バイトに揃っていなければ何もせず偽。
    pub(crate) fn put_u32(&mut self, addr: usize, value: u32) -> bool {
        let Some(at) = self.offset(addr, size_of::<u32>(), align_of::<u32>()) else {
            return false;
        };
        let target = self
            .words
            .as_mut_ptr()
            .cast::<u8>()
            .wrapping_add(at)
            .cast::<u32>();
        // SAFETY: `offset` が `at..at + 4` を作業領域の確保の内と、`target` が 4 バイトに揃うことを
        // 確かめた。ポインタは `&mut self` の Vec から取ったので、書く間にほかの参照は無い。
        // u32 は Drop を持たないので、前の値を読まずに上書きしてよい。
        unsafe { target.write(value) };
        true
    }
}

/// テストの側から作業領域を書く (JS が型付き配列で書くのと同じ)。読むのは `bytes` で足りる。
#[cfg(test)]
impl Scratch {
    /// `addr` から `data` を書く。作業領域の外なら何もせず偽。
    pub(crate) fn poke(&mut self, addr: usize, data: &[u8]) -> bool {
        let Some(at) = self.offset(addr, data.len(), 1) else {
            return false;
        };
        let target = self.words.as_mut_ptr().cast::<u8>().wrapping_add(at);
        // SAFETY: `offset` が `at..at + data.len()` を作業領域の確保の内と確かめた。ポインタは `&mut self` の
        // Vec から取ったので、書く間にほかの参照は無い。`data` はテストの側の値で、作業領域と重ならない
        // (作業領域を指す参照は `&mut self` の外に無い)。u8 は Drop を持たない。
        unsafe { target.copy_from_nonoverlapping(data.as_ptr(), data.len()) };
        true
    }
}
