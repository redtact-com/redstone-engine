//! 世界の表と、窓口の関数の中身。unsafe を使わない (読み書きは scratch.rs を通す)。
//!
//! - 世界はハンドル (u32) で持つ。下の 16 ビットが席の番号 + 1、上の 16 ビットが席の世代である。
//!   捨てた席は使い回し、世代を 1 つ進める。世代が最後 (0xFFFF) の世界を捨てた席は、使い回さずに退かせる
//!   (世代が一周しない)。だから、同じハンドルが 2 つの世界を指すことは無く、捨てた世界の古いハンドルは
//!   いつまでも HANDLE を返す。0 は、どの世界のハンドルにもならない。
//! - 限り: 同時に持てる世界は 65,535 個 (席の数)。作れる世界の延べの数は 65,535 席 × 65,536 世代で、
//!   使い切ると `rse_world_new` は 0 を返す (インスタンスを読み直すまで)。
//! - 表は thread_local に 1 つ持つ。wasm は単一スレッドなので 1 つになり、native のテストは
//!   スレッドごとに別の表になる。

use crate::scratch::Scratch;
use crate::{CellRec, input_kind, load_mode, status};
use rse_engine::rse_core::state::{ParseError, STATE_COUNT};
use rse_engine::{Config, Input, InputSection, Pos, StateId, Status, World, new_world};
use std::cell::RefCell;

/// 席の数 (席の番号 + 1 が 16 ビットに入る数)。同時に持てる世界の数でもある。
const MAX_SLOTS: usize = 0xFFFF;

struct Entry {
    world: World,
    /// `rse_changes` が書き出す列。使い回す。
    changes: Vec<CellRec>,
}

struct Slot {
    generation: u16,
    entry: Option<Box<Entry>>,
}

pub(crate) struct Registry {
    slots: Vec<Slot>,
    /// 空いた席の番号。
    free: Vec<usize>,
    scratch: Scratch,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = const {
        RefCell::new(Registry {
            slots: Vec::new(),
            free: Vec::new(),
            scratch: Scratch::new(),
        })
    };
}

/// 表を借りて `f` を呼ぶ。窓口の関数は呼び返しを持たないので、借用は重ならない。
pub(crate) fn with<T>(f: impl FnOnce(&mut Registry) -> T) -> T {
    REGISTRY.with_borrow_mut(f)
}

fn handle_of(index: usize, generation: u16) -> u32 {
    ((generation as u32) << 16) | (index as u32 + 1)
}

/// ハンドルの指す世界。作っていない・もう捨てた・世代が違うなら `None`。
fn find(slots: &mut [Slot], handle: u32) -> Option<&mut Entry> {
    let index = (handle & 0xFFFF).checked_sub(1)? as usize;
    let slot = slots.get_mut(index)?;
    if slot.generation != (handle >> 16) as u16 {
        return None;
    }
    slot.entry.as_deref_mut()
}

impl Registry {
    fn entry(&mut self, handle: u32) -> Option<&mut Entry> {
        find(&mut self.slots, handle)
    }

    pub(crate) fn scratch(&mut self, len: u32) -> *mut u8 {
        self.scratch
            .ensure(len as usize)
            .unwrap_or(core::ptr::null_mut())
    }

    /// 作業領域の今の大きさ (バイト)。
    #[cfg(test)]
    pub(crate) fn scratch_len(&self) -> usize {
        self.scratch.len_bytes()
    }

    /// テスト: 作業領域 (と、その確かめ) を借りる。
    #[cfg(test)]
    pub(crate) fn scratch_mut(&mut self) -> &mut Scratch {
        &mut self.scratch
    }

    /// テスト: `rse_changes` が返した番地と件数が、その世界の変化の列そのものなら、写しを返す。
    #[cfg(test)]
    pub(crate) fn changes_at(
        &mut self,
        handle: u32,
        ptr: *const CellRec,
        n: usize,
    ) -> Option<Vec<CellRec>> {
        let entry = self.entry(handle)?;
        (entry.changes.as_ptr() == ptr && entry.changes.len() == n).then(|| entry.changes.clone())
    }

    pub(crate) fn world_new(&mut self) -> u32 {
        let index = match self.free.pop() {
            Some(index) => index,
            None if self.slots.len() < MAX_SLOTS => {
                self.slots.push(Slot {
                    generation: 0,
                    entry: None,
                });
                self.slots.len() - 1
            }
            None => return 0,
        };
        let slot = &mut self.slots[index];
        slot.entry = Some(Box::new(Entry {
            world: new_world(Config::default()),
            changes: Vec::new(),
        }));
        handle_of(index, slot.generation)
    }

    pub(crate) fn world_free(&mut self, handle: u32) -> u32 {
        if self.entry(handle).is_none() {
            return status::HANDLE;
        }
        let index = (handle & 0xFFFF) as usize - 1;
        let slot = &mut self.slots[index];
        slot.entry = None;
        // 世代が最後なら、席を退かせる (空いた席の一覧に戻さない)。世代は一周させない。
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(index);
        }
        Status::OK.0
    }

    pub(crate) fn resolve(&mut self, addr: usize, len: u32, out: usize) -> u32 {
        if !self.scratch.fits_u32(out) {
            return status::ARGUMENT;
        }
        let Some(bytes) = self.scratch.bytes(addr, len as usize) else {
            return status::ARGUMENT;
        };
        let parsed = match core::str::from_utf8(bytes) {
            Ok(text) => StateId::parse(text),
            Err(_) => Err(ParseError::Syntax),
        };
        match parsed {
            Ok(state) => {
                self.scratch.put_u32(out, state.0 as u32);
                Status::OK.0
            }
            // 失敗した時は out に書かない。
            Err(ParseError::UnknownBlock) => status::UNKNOWN_BLOCK,
            Err(ParseError::UnknownProperty) => status::UNKNOWN_PROPERTY,
            Err(ParseError::UnknownValue) => status::UNKNOWN_VALUE,
            Err(ParseError::Syntax) => status::SYNTAX,
        }
    }

    pub(crate) fn load(&mut self, handle: u32, addr: usize, n: u32, mode: u32) -> u32 {
        let Registry { slots, scratch, .. } = self;
        let Some(entry) = find(slots, handle) else {
            return status::HANDLE;
        };
        match mode {
            load_mode::SETTLED => {}
            // 席だけ (19 §4.2 の「組み直し」)。黙って「そのまま」で置かない。
            load_mode::REBUILD => return Status::UNIMPLEMENTED.0,
            _ => return status::ARGUMENT,
        }
        let Some(cells) = scratch.cells(addr, n as usize) else {
            return status::ARGUMENT;
        };
        let world = &mut entry.world;
        // 先に全部を確かめ、確かめられる誤りでは 1 つも置かない。
        if cells.iter().any(|c| c.state as usize >= STATE_COUNT) {
            return status::ARGUMENT;
        }
        // 高さの外と、格子の枠の外 (今の格子と列を合わせると 1 辺 256 セクションを超える) は、置く前に
        // rse-core で確かめ、1 つも書かずに断る (全部か無しか)。格子もセクションも増やさない。
        let cell = |c: &CellRec| (Pos::new(c.x, c.y, c.z), StateId(c.state as u16));
        if !world.can_set_all_raw(cells.iter().map(cell)) {
            return status::OUT_OF_WORLD;
        }
        for c in cells {
            let (pos, state) = cell(c);
            let placed = world.set_state_raw(pos, state);
            // 上で全部書けると確かめたので、ここで断られることは無い。
            debug_assert!(placed, "can_set_all_raw が通したのに置けない");
        }
        Status::OK.0
    }

    pub(crate) fn step(&mut self, handle: u32, n: u32) -> u32 {
        let Some(entry) = self.entry(handle) else {
            return status::HANDLE;
        };
        let mut out = Status::OK;
        for _ in 0..n {
            out |= entry.world.tick();
        }
        out.0
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn input(
        &mut self,
        handle: u32,
        kind: u32,
        section: u32,
        x: i32,
        y: i32,
        z: i32,
        arg: u32,
    ) -> u32 {
        let Some(entry) = self.entry(handle) else {
            return status::HANDLE;
        };
        // 区間の番号は rse-core の InputSection の値 (19 §4.1)。
        let section = match section {
            0 => InputSection::B5,
            1 => InputSection::C,
            2 => InputSection::A,
            3 => InputSection::B1,
            _ => return status::ARGUMENT,
        };
        let pos = Pos::new(x, y, z);
        let input = match kind {
            // 使う入力は arg を読まない。読まない値が 0 でなければ断る。
            input_kind::USE if arg == 0 => Input::Use { pos },
            input_kind::SET_BLOCK => {
                let (state, strict) = (arg & 0xFFFF, arg >> 16);
                if state as usize >= STATE_COUNT || strict > 1 {
                    return status::ARGUMENT;
                }
                Input::SetBlock {
                    pos,
                    state: StateId(state as u16),
                    strict: strict == 1,
                }
            }
            _ => return status::ARGUMENT,
        };
        entry.world.input(section, input).status.0
    }

    pub(crate) fn changes(&mut self, handle: u32, len_out: usize) -> *const CellRec {
        // 件数を書けない時は、変化を取り出さずに返す (取り出した変化を捨てない)。
        if !self.scratch.fits_u32(len_out) {
            return core::ptr::null();
        }
        let Registry { slots, scratch, .. } = self;
        let Some(Entry { world, changes }) = find(slots, handle) else {
            scratch.put_u32(len_out, 0);
            return core::ptr::null();
        };
        changes.clear();
        changes.extend(world.drain_changes().iter().map(|c| CellRec {
            x: c.pos.x,
            y: c.pos.y,
            z: c.pos.z,
            state: c.state.0 as u32,
        }));
        scratch.put_u32(len_out, changes.len() as u32);
        // 空の列でも、Vec の先頭は null でなく揃っている。JS は null を失敗として読む。
        changes.as_ptr()
    }
}
