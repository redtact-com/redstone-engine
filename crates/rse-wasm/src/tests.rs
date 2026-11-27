//! 窓口の 8 関数を、native で JS と同じ呼び方 (作業領域に書いて番地を渡す) で確かめる。
//!
//! テストはスレッドごとに別の世界の表を持つ (registry.rs の thread_local)。

use super::*;
use crate::registry;
use rse_engine::rse_core::state::STATE_COUNT;
use rse_engine::{Config, Input, InputSection, Pos, StateId, Status, new_world};

const A: u32 = 2;

/// `ptr` に `data` を書く (JS が作業領域に書くのと同じ)。作業領域の外なら panic (テストの誤り)。
fn poke(ptr: *mut u8, data: &[u8]) {
    assert!(
        registry::with(|r| r.scratch_mut().poke(ptr.addr(), data)),
        "作業領域の外"
    );
}

/// `ptr` の u32 を読む。作業領域の外なら panic。
fn peek_u32(ptr: *const u8) -> u32 {
    registry::with(|r| {
        let bytes = r.scratch_mut().bytes(ptr.addr(), 4).expect("作業領域の外");
        u32::from_le_bytes(bytes.try_into().unwrap())
    })
}

/// `rse_changes` が返した番地と件数を、その世界の変化の列と突き合わせてから写す。
fn peek_cells(handle: u32, ptr: *const CellRec, n: usize) -> Vec<CellRec> {
    registry::with(|r| r.changes_at(handle, ptr, n)).expect("変化の列でない番地か件数")
}

/// JS の resolve と同じ: 作業領域の先頭 4 バイトを out、その後ろを名前にする。
fn resolve(name: &str) -> Result<u32, u32> {
    let bytes = name.as_bytes();
    let base = rse_scratch(4 + bytes.len() as u32);
    assert!(!base.is_null());
    poke(base.wrapping_add(4), bytes);
    poke(base, &u32::MAX.to_le_bytes());
    let code = rse_resolve(base.wrapping_add(4), bytes.len() as u32, base.cast());
    if code == 0 {
        Ok(peek_u32(base))
    } else {
        // 失敗した時は out に書かない。
        assert_eq!(peek_u32(base), u32::MAX, "{name}");
        Err(code)
    }
}

fn id(name: &str) -> u32 {
    resolve(name).unwrap_or_else(|code| panic!("{name}: {code:#x}"))
}

fn cell(x: i32, y: i32, z: i32, name: &str) -> CellRec {
    CellRec {
        x,
        y,
        z,
        state: id(name),
    }
}

/// JS の load と同じ: 作業領域にマスの記録を書いて渡す。
fn load(handle: u32, cells: &[CellRec], mode: u32) -> u32 {
    let base = rse_scratch((cells.len() * 16) as u32);
    let bytes: Vec<u8> = cells
        .iter()
        .flat_map(|c| {
            [c.x, c.y, c.z, c.state as i32]
                .into_iter()
                .flat_map(i32::to_le_bytes)
        })
        .collect();
    poke(base, &bytes);
    rse_load(handle, base.cast(), cells.len() as u32, mode)
}

/// JS の changes と同じ: 作業領域の先頭 4 バイトに件数を受ける。
fn changes(handle: u32) -> Option<Vec<CellRec>> {
    let base = rse_scratch(4);
    let ptr = rse_changes(handle, base.cast());
    let n = peek_u32(base) as usize;
    if ptr.is_null() {
        assert_eq!(n, 0);
        None
    } else {
        Some(peek_cells(handle, ptr, n))
    }
}

/// 石の床の上に、レバー → ダスト 3 本 → ランプ (fixture の lever-wire-lamp と同じ盤面)。
fn circuit() -> Vec<CellRec> {
    let wire = "redstone_wire[east=side,north=none,power=0,south=none,west=side]";
    let mut out: Vec<CellRec> = (0..=4)
        .flat_map(|x| [cell(x, 0, 0, "stone"), cell(x, 0, 1, "stone")])
        .collect();
    out.push(cell(
        0,
        1,
        0,
        "lever[face=floor,facing=north,powered=false]",
    ));
    for x in 1..=3 {
        out.push(cell(x, 1, 0, wire));
    }
    out.push(cell(4, 1, 0, "redstone_lamp[lit=false]"));
    out
}

fn use_lever(handle: u32) -> u32 {
    rse_input(handle, input_kind::USE, A, 0, 1, 0, 0)
}

fn lamp_of(changes: &[CellRec]) -> Option<u32> {
    changes
        .iter()
        .find(|c| (c.x, c.y, c.z) == (4, 1, 0))
        .map(|c| c.state)
}

#[test]
fn abi_version_is_one() {
    assert_eq!(rse_abi_version(), 1);
    assert_eq!(ABI_VERSION, 1);
}

// 窓口の状態コードは上の 16 ビットにあり、エンジンの状態コードと重ならない。値は境界を越えるので固定する。
#[test]
fn status_bits_are_fixed_and_disjoint() {
    let bits: Vec<u32> = status::ALL.iter().map(|(b, _)| *b).collect();
    assert_eq!(bits, (16..23).map(|n| 1u32 << n).collect::<Vec<_>>());
    let engine = Status::ALL.iter().fold(0, |acc, (s, _)| acc | s.0);
    assert!(engine <= 0xFFFF);
    for (bit, name) in status::ALL {
        assert_eq!(bit & 0xFFFF, 0, "{name}");
    }
}

#[test]
fn worlds_are_created_and_freed_by_handle() {
    let a = rse_world_new();
    let b = rse_world_new();
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);
    assert_eq!(rse_world_free(a), 0);
    // 2 回捨てると HANDLE。0 と、作っていない番号も HANDLE。
    assert_eq!(rse_world_free(a), status::HANDLE);
    assert_eq!(rse_world_free(0), status::HANDLE);
    assert_eq!(rse_world_free(0xFFFF), status::HANDLE);
    // 捨てた席を使い回しても、古いハンドルは新しい世界に当たらない (世代が進む)。
    let c = rse_world_new();
    assert_eq!(c & 0xFFFF, a & 0xFFFF);
    assert_ne!(c, a);
    for code in [
        rse_step(a, 1),
        use_lever(a),
        load(a, &[], load_mode::SETTLED),
    ] {
        assert_eq!(code, status::HANDLE);
    }
    assert_eq!(changes(a), None);
    assert_eq!(rse_step(c, 1), 0);
    assert_eq!(rse_world_free(b), 0);
    assert_eq!(rse_world_free(c), 0);
}

#[test]
fn names_resolve_to_the_same_ids_as_the_engine() {
    for s in 0..STATE_COUNT {
        let state = StateId(s as u16);
        assert_eq!(resolve(&state.to_string()), Ok(s as u32), "{state}");
    }
    // 先頭の minecraft: と、書いていないプロパティ (既定の値) は StateId::parse のとおり。
    assert_eq!(
        resolve("minecraft:redstone_lamp"),
        resolve("redstone_lamp[lit=false]")
    );
    assert_eq!(
        resolve("lever[powered=true]"),
        resolve("lever[face=wall,facing=north,powered=true]")
    );
}

#[test]
fn bad_names_say_why_and_write_nothing() {
    assert_eq!(resolve("repeater"), Err(status::UNKNOWN_BLOCK));
    assert_eq!(resolve(""), Err(status::UNKNOWN_BLOCK));
    assert_eq!(
        resolve("redstone_lamp[on=true]"),
        Err(status::UNKNOWN_PROPERTY)
    );
    assert_eq!(
        resolve("redstone_lamp[lit=yes]"),
        Err(status::UNKNOWN_VALUE)
    );
    assert_eq!(resolve("redstone_lamp[lit=true"), Err(status::SYNTAX));
    // UTF-8 でないバイト列は形の崩れ。
    let base = rse_scratch(8);
    poke(base.wrapping_add(4), &[0xFF, 0xFE]);
    assert_eq!(
        rse_resolve(base.wrapping_add(4), 2, base.cast()),
        status::SYNTAX
    );
}

// ポインタは作業領域の内でなければ断り、何も読み書きしない。
#[test]
fn pointers_outside_the_scratch_are_refused() {
    // 作業領域は縮まないので、同じスレッドで前に回ったテストの分まで広いことがある。端は今の大きさで決める。
    let base = rse_scratch(64);
    let size = registry::with(|r| r.scratch_len());
    poke(base, b"\0\0\0\0stone");
    let name = base.wrapping_add(4);
    let out = base.cast::<u32>();
    assert_eq!(rse_resolve(name, 5, out), 0);
    assert_eq!(peek_u32(base), id("stone"));
    // 名前が作業領域の外へはみ出す。末尾ちょうどまでは読める。
    assert_eq!(
        rse_resolve(name, (size - 4) as u32, out),
        status::UNKNOWN_BLOCK
    );
    assert_eq!(rse_resolve(name, (size - 3) as u32, out), status::ARGUMENT);
    assert_eq!(rse_resolve(name, u32::MAX, out), status::ARGUMENT);
    // 作業領域と関係の無い所。
    let elsewhere = [0u8; 8];
    assert_eq!(rse_resolve(elsewhere.as_ptr(), 1, out), status::ARGUMENT);
    assert_eq!(
        rse_resolve(name, 5, elsewhere.as_ptr().cast_mut().cast()),
        status::ARGUMENT
    );
    // out が 4 バイトに揃っていない・末尾をはみ出す。
    assert_eq!(
        rse_resolve(name, 5, base.wrapping_add(1).cast()),
        status::ARGUMENT
    );
    assert_eq!(rse_resolve(name, 5, base.wrapping_add(size - 4).cast()), 0);
    assert_eq!(
        rse_resolve(name, 5, base.wrapping_add(size).cast()),
        status::ARGUMENT
    );
    // null。
    assert_eq!(rse_resolve(core::ptr::null(), 0, out), status::ARGUMENT);
    // 読み込みの列が揃っていない・はみ出す・件数のかけ算があふれる。
    let h = rse_world_new();
    let recs = base.cast::<CellRec>();
    assert_eq!(
        rse_load(h, base.wrapping_add(2).cast(), 1, 0),
        status::ARGUMENT
    );
    assert_eq!(
        rse_load(h, recs, (size / 16 + 1) as u32, 0),
        status::ARGUMENT
    );
    assert_eq!(rse_load(h, recs, u32::MAX, 0), status::ARGUMENT);
    // changes の件数の書き先が外なら、null を返し、変化を取り出さない (捨てない)。
    assert_eq!(load(h, &[cell(0, 0, 0, "stone")], 0), 0);
    assert!(rse_changes(h, elsewhere.as_ptr().cast_mut().cast()).is_null());
    assert_eq!(changes(h).unwrap().len(), 1);
    assert_eq!(rse_world_free(h), 0);
}

// 作業領域は縮まない。広げても前の中身は残り、広げた分は 0 で埋まる。
#[test]
fn scratch_grows_and_keeps_contents() {
    let base = rse_scratch(4);
    poke(base, &7u32.to_le_bytes());
    let wider = rse_scratch(1 << 16);
    assert!(!wider.is_null());
    assert_eq!(wider.addr() % 8, 0);
    assert_eq!(peek_u32(wider), 7);
    assert_eq!(peek_u32(wider.wrapping_add((1 << 16) - 4)), 0);
    // 小さく頼んでも縮めない。番地も変わらない。
    assert_eq!(rse_scratch(0), wider);
}

#[test]
fn load_places_cells_without_updates() {
    let h = rse_world_new();
    let board = circuit();
    assert_eq!(load(h, &board, load_mode::SETTLED), 0);
    // 置いたマスは変化の列に載る (置いた順)。更新を出さないので、ほかのマスは変わらない。
    assert_eq!(changes(h).unwrap(), board);
    assert_eq!(changes(h).unwrap(), []);
    // 静かな盤面は 8 tick 空回ししても変わらない (19 §4.2 の点検)。
    assert_eq!(rse_step(h, 8), 0);
    assert_eq!(changes(h).unwrap(), []);
    assert_eq!(rse_world_free(h), 0);
}

#[test]
fn load_refuses_bad_cells_before_placing_any() {
    let h = rse_world_new();
    let stone = id("stone");
    let ok = CellRec {
        x: 0,
        y: 0,
        z: 0,
        state: stone,
    };
    let bad_state = CellRec {
        state: STATE_COUNT as u32,
        ..ok
    };
    let too_high = CellRec { y: 320, ..ok };
    assert_eq!(load(h, &[ok, bad_state], 0), status::ARGUMENT);
    assert_eq!(load(h, &[ok, too_high], 0), status::OUT_OF_WORLD);
    assert_eq!(changes(h).unwrap(), []);
    // 組み直しは席だけ。黙って「そのまま」で置かない。知らない型は ARGUMENT。
    assert_eq!(load(h, &[ok], load_mode::REBUILD), Status::UNIMPLEMENTED.0);
    assert_eq!(load(h, &[ok], 2), status::ARGUMENT);
    assert_eq!(changes(h).unwrap(), []);
    // 格子の枠 (離れすぎた 2 点) は置いてみるまで分からない。置けなかったら、前に置いたマスを戻す
    // (全部か無しか)。前からあったマスも、同じ列で書き換えた分は前の状態に戻る。
    let lamp = CellRec {
        state: id("redstone_lamp"),
        ..ok
    };
    assert_eq!(load(h, &[lamp], 0), 0);
    changes(h);
    let far = CellRec { x: 16 * 256, ..ok };
    let next = CellRec { x: 1, ..ok };
    assert_eq!(load(h, &[ok, next, ok, far], 0), status::OUT_OF_WORLD);
    assert_eq!(changes(h).unwrap(), []);
    let ok_far = CellRec { x: 16 * 255, ..ok };
    assert_eq!(load(h, &[next, ok_far], 0), 0);
    assert_eq!(changes(h).unwrap(), [next, ok_far]);
    assert_eq!(rse_world_free(h), 0);
}

// 同じ席を使い回して世代が最後 (0xFFFF) まで来たら、その席は退かせる。古いハンドルはどれも当たらない。
#[test]
fn slots_retire_instead_of_wrapping_the_generation() {
    let first = rse_world_new();
    let index = first & 0xFFFF;
    let mut last = first;
    let mut count = 0;
    loop {
        assert_eq!(rse_world_free(last), 0);
        let next = rse_world_new();
        count += 1;
        if next & 0xFFFF != index {
            // 前の席は退いた。最後に捨てた世界の世代は 0xFFFF。
            assert_eq!(last >> 16, 0xFFFF);
            assert_eq!(rse_world_free(next), 0);
            break;
        }
        assert_eq!(next >> 16, (last >> 16) + 1);
        last = next;
        assert!(count <= 0x10000, "世代が一周した");
    }
    // 退いた席の古いハンドルは、どれも HANDLE を返す。世代 0 の形のハンドルも当たらない。
    for stale in [first, last, index] {
        assert_eq!(rse_step(stale, 1), status::HANDLE);
        assert_eq!(rse_world_free(stale), status::HANDLE);
    }
}

// レバーを入れるとその場でランプが点き、戻すと 4gt 後に消える (V11 §4.1)。
#[test]
fn step_input_and_changes_follow_the_engine() {
    let h = rse_world_new();
    assert_eq!(load(h, &circuit(), 0), 0);
    changes(h);
    assert_eq!(rse_step(h, 2), 0);
    assert_eq!(use_lever(h), 0);
    let on = changes(h).unwrap();
    assert_eq!(on.len(), 5);
    assert_eq!(lamp_of(&on), Some(id("redstone_lamp[lit=true]")));
    assert_eq!(use_lever(h), 0);
    let off = changes(h).unwrap();
    assert_eq!(lamp_of(&off), None);
    assert_eq!(rse_step(h, 3), 0);
    assert_eq!(changes(h).unwrap(), []);
    assert_eq!(rse_step(h, 1), 0);
    assert_eq!(
        changes(h).unwrap(),
        [cell(4, 1, 0, "redstone_lamp[lit=false]")]
    );
    // 0 tick は何もしない。
    assert_eq!(rse_step(h, 0), 0);
    assert_eq!(rse_world_free(h), 0);
}

// 窓口を通した結果は、rse-engine を直に呼んだ結果と、tick ごとの変化の列まで同じになる。
#[test]
fn abi_matches_the_engine_tick_by_tick() {
    let h = rse_world_new();
    let mut direct = new_world(Config::default());
    let board = circuit();
    assert_eq!(load(h, &board, 0), 0);
    for c in &board {
        assert!(direct.set_state_raw(Pos::new(c.x, c.y, c.z), StateId(c.state as u16)));
    }
    let drain = |w: &mut rse_engine::World| -> Vec<CellRec> {
        w.drain_changes()
            .iter()
            .map(|c| CellRec {
                x: c.pos.x,
                y: c.pos.y,
                z: c.pos.z,
                state: c.state.0 as u32,
            })
            .collect()
    };
    assert_eq!(changes(h).unwrap(), drain(&mut direct));
    for t in 1..=18 {
        assert_eq!(rse_step(h, 1), direct.tick().0);
        if t == 2 || t == 10 {
            let lever = Input::Use {
                pos: Pos::new(0, 1, 0),
            };
            assert_eq!(use_lever(h), direct.input(InputSection::A, lever).status.0);
        }
        assert_eq!(changes(h).unwrap(), drain(&mut direct), "tick {t}");
    }
    assert_eq!(rse_world_free(h), 0);
}

#[test]
fn worlds_are_independent() {
    let (a, b) = (rse_world_new(), rse_world_new());
    assert_eq!(load(a, &circuit(), 0), 0);
    assert_eq!(load(b, &circuit(), 0), 0);
    changes(a);
    changes(b);
    assert_eq!(use_lever(a), 0);
    assert_eq!(changes(a).unwrap().len(), 5);
    assert_eq!(changes(b).unwrap(), []);
    // a を捨てても b は動く。
    assert_eq!(rse_world_free(a), 0);
    assert_eq!(use_lever(b), 0);
    assert_eq!(
        lamp_of(&changes(b).unwrap()),
        Some(id("redstone_lamp[lit=true]"))
    );
    assert_eq!(rse_world_free(b), 0);
}

#[test]
fn input_arguments_are_checked() {
    let h = rse_world_new();
    assert_eq!(load(h, &circuit(), 0), 0);
    changes(h);
    // 区間の番号は 0〜3 だけ。知らない種類も断る。使う入力の arg は 0 でなければ断る。
    assert_eq!(
        rse_input(h, input_kind::USE, 4, 0, 1, 0, 0),
        status::ARGUMENT
    );
    assert_eq!(rse_input(h, 9, A, 0, 1, 0, 0), status::ARGUMENT);
    assert_eq!(
        rse_input(h, input_kind::USE, A, 0, 1, 0, 1),
        status::ARGUMENT
    );
    assert_eq!(changes(h).unwrap(), []);
    // どの区間でも、使う入力はその場で連鎖を回し切る (区間の印は順を変えない)。
    // 4 回入れるとレバーとダストは元に戻り、ランプだけが点いたまま残る (消灯は 4gt 後。V11 §4.1)。
    for section in 0..4 {
        assert_eq!(rse_input(h, input_kind::USE, section, 0, 1, 0, 0), 0);
    }
    assert_eq!(
        changes(h).unwrap(),
        [cell(4, 1, 0, "redstone_lamp[lit=true]")]
    );
    // setblock は席だけ。黙って捨てずに UNIMPLEMENTED を返す。StateId が表の外・strict が 0/1 でなければ ARGUMENT。
    let stone = id("stone");
    assert_eq!(
        rse_input(h, input_kind::SET_BLOCK, 1, 5, 1, 0, stone | 1 << 16),
        Status::UNIMPLEMENTED.0
    );
    assert_eq!(
        rse_input(h, input_kind::SET_BLOCK, 1, 5, 1, 0, STATE_COUNT as u32),
        status::ARGUMENT
    );
    assert_eq!(
        rse_input(h, input_kind::SET_BLOCK, 1, 5, 1, 0, stone | 2 << 16),
        status::ARGUMENT
    );
    assert_eq!(changes(h).unwrap(), []);
    // 何もしない所を使っても、状態コードは立たない。
    assert_eq!(rse_input(h, input_kind::USE, A, 0, 0, 0, 0), 0);
    assert_eq!(rse_world_free(h), 0);
}
