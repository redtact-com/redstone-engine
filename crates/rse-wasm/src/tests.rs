//! 窓口の 10 関数を、native で JS と同じ呼び方 (作業領域に書いて番地を渡す) で確かめる。
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

// 版の字面はここに書かない (上げた時に直す所を ABI_VERSION の 1 か所にする)。engine-js は読み込んだ wasm の
// rse_abi_version を自分の ABI_VERSION と比べるので、Rust と TS の版が違えば engine-js のテストが落ちる。
#[test]
fn abi_version_is_the_constant() {
    assert_eq!(rse_abi_version(), ABI_VERSION);
}

// 入力の種類の番号は境界を越えるので固定する (engine-js の INPUT_KIND と同じ値)。
#[test]
fn input_kinds_are_fixed() {
    use input_kind::*;
    assert_eq!([USE, SET_BLOCK, PLACE, BREAK], [0, 1, 2, 3]);
}

// 書き出しの 10 関数の形 (引数と返り値の型) を固定する。型が違えば組めない。
// wasm の書き出しの全部 (関数の顔ぶれ・memory・global) は、engine-js のテストが組んだ wasm で見る。
#[test]
fn exports_keep_their_shapes() {
    macro_rules! shapes {
        ($($name:ident: $ty:ty),* $(,)?) => {
            [$((stringify!($name), $name as $ty as usize)),*]
        };
    }
    let all = shapes![
        rse_abi_version: extern "C" fn() -> u32,
        rse_scratch: extern "C" fn(u32) -> *mut u8,
        rse_world_new: extern "C" fn() -> u32,
        rse_world_free: extern "C" fn(u32) -> u32,
        rse_resolve: extern "C" fn(*const u8, u32, *mut u32) -> u32,
        rse_load: extern "C" fn(u32, *const CellRec, u32, u32) -> u32,
        rse_step: extern "C" fn(u32, u32) -> u32,
        rse_input: extern "C" fn(u32, u32, u32, i32, i32, i32, u32) -> u32,
        rse_changes: extern "C" fn(u32, *mut u32) -> *const CellRec,
        rse_game_time: extern "C" fn(u32) -> i64,
    ];
    assert_eq!(all.len(), 10);
    // 名前は全部 rse_ で始まり、別々の関数を指す。
    for (i, (name, at)) in all.iter().enumerate() {
        assert!(name.starts_with("rse_"), "{name}");
        assert!(all[..i].iter().all(|(_, other)| other != at), "{name}");
    }
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
        rse_input(a, input_kind::PLACE, A, 0, 1, 0, id("stone")),
        rse_input(a, input_kind::BREAK, A, 0, 1, 0, 0),
        load(a, &[], load_mode::SETTLED),
    ] {
        assert_eq!(code, status::HANDLE);
    }
    assert_eq!(changes(a), None);
    // 時計はハンドルが無ければ -1 (0 と、捨てた世界と、作っていない番号)。
    for stale in [a, 0, 0xFFFF] {
        assert_eq!(rse_game_time(stale), -1);
    }
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
    // 格子の枠 (今の格子と列を合わせて 1 辺 256 セクション) も、置く前に確かめて 1 つも書かない。
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

// 断った読み込みは、格子の広がりも残さない。何も無い世界で通る読み込みは、断られた後でも通る。
// (置いてから戻す形では、断った読み込みが 0〜255 セクションまで格子を広げたまま残り、後の -10 が枠を超えた。)
#[test]
fn refused_load_leaves_no_grid_behind() {
    let h = rse_world_new();
    let stone = id("stone");
    let at = |x: i32| CellRec {
        x,
        y: 0,
        z: 0,
        state: stone,
    };
    assert_eq!(
        load(h, &[at(0), at(16 * 255), at(16 * 300)], 0),
        status::OUT_OF_WORLD
    );
    assert_eq!(changes(h).unwrap(), []);
    assert_eq!(load(h, &[at(-16 * 10)], 0), 0);
    assert_eq!(changes(h).unwrap(), [at(-16 * 10)]);
    // 今の格子 (-10 のセクション) と合わせて枠を超える列も、1 つも書かずに断る。
    assert_eq!(load(h, &[at(0), at(16 * 246)], 0), status::OUT_OF_WORLD);
    assert_eq!(load(h, &[at(0), at(16 * 245)], 0), 0);
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

// ---- 置く・壊す (input_kind の 2 と 3) と時計 (rse_game_time)。版 2 で足した ----

const WIRE_CROSS: &str = "redstone_wire[east=side,north=side,power=0,south=side,west=side]";
const WIRE_LINE_X: &str = "redstone_wire[east=side,north=none,power=0,south=none,west=side]";

fn place(handle: u32, x: i32, y: i32, z: i32, name: &str) -> u32 {
    rse_input(handle, input_kind::PLACE, A, x, y, z, id(name))
}

fn break_at(handle: u32, x: i32, y: i32, z: i32) -> u32 {
    rse_input(handle, input_kind::BREAK, A, x, y, z, 0)
}

/// 石の床 (y=0) の x 0〜4・z 0〜2。
fn floor() -> Vec<CellRec> {
    (0..=4)
        .flat_map(|x| (0..=2).map(move |z| cell(x, 0, z, "stone")))
        .collect()
}

/// 変化の列を、座標の順に並べた (座標, 名前) にする。
fn named(changes: &[CellRec]) -> Vec<((i32, i32, i32), String)> {
    let mut out: Vec<_> = changes
        .iter()
        .map(|c| ((c.x, c.y, c.z), StateId(c.state as u16).to_string()))
        .collect();
    out.sort();
    out
}

// 置く・壊すは rse-core の入力そのもの。ダストの形は置いた時の規則と形状更新が決める (V06 §4.3・§4.4)。
// 置けない所 (置き換えられない・支えが無い) と空気を壊す、は何もせずに 0 を返す。
#[test]
fn place_and_break_go_through_the_engine() {
    let h = rse_world_new();
    assert_eq!(load(h, &floor(), 0), 0);
    changes(h);
    // 孤立したダストは十字で置かれる。要求のプロパティ (既定の状態) は読まない。
    assert_eq!(place(h, 1, 1, 1, "redstone_wire"), 0);
    assert_eq!(
        named(&changes(h).unwrap()),
        [((1, 1, 1), WIRE_CROSS.to_string())]
    );
    // 隣に置くと、どちらも東西の直線になる。
    assert_eq!(place(h, 2, 1, 1, WIRE_CROSS), 0);
    assert_eq!(
        named(&changes(h).unwrap()),
        [
            ((1, 1, 1), WIRE_LINE_X.to_string()),
            ((2, 1, 1), WIRE_LINE_X.to_string())
        ]
    );
    // 壊すと空気になり、残ったダストは十字に戻る。
    assert_eq!(break_at(h, 2, 1, 1), 0);
    assert_eq!(
        named(&changes(h).unwrap()),
        [
            ((1, 1, 1), WIRE_CROSS.to_string()),
            ((2, 1, 1), "air".to_string())
        ]
    );
    // 置き換えられない所 (石・ダスト)・支えの無い所 (下が空気)・空気を壊す、は何もしない。
    assert_eq!(place(h, 0, 0, 0, "redstone_lamp"), 0);
    assert_eq!(place(h, 1, 1, 1, "stone"), 0);
    assert_eq!(place(h, 1, 3, 1, "redstone_wire"), 0);
    assert_eq!(place(h, 1, 3, 1, "lever[face=floor,facing=east]"), 0);
    assert_eq!(break_at(h, 3, 1, 1), 0);
    assert_eq!(changes(h).unwrap(), []);
    // 規則の無い要求 (入った状態のレバー) は、何も書かずに UNIMPLEMENTED を返す。
    assert_eq!(
        place(h, 3, 1, 1, "lever[face=floor,facing=east,powered=true]"),
        Status::UNIMPLEMENTED.0
    );
    assert_eq!(changes(h).unwrap(), []);
    // 支えのある所なら、石もランプもレバーも置ける。
    assert_eq!(place(h, 3, 1, 1, "lever[face=floor,facing=east]"), 0);
    assert_eq!(place(h, 4, 1, 1, "redstone_lamp"), 0);
    assert_eq!(place(h, 1, 5, 1, "stone"), 0);
    assert_eq!(
        named(&changes(h).unwrap()),
        [
            ((1, 5, 1), "stone".to_string()),
            (
                (3, 1, 1),
                "lever[face=floor,facing=east,powered=false]".to_string()
            ),
            ((4, 1, 1), "redstone_lamp[lit=false]".to_string()),
        ]
    );
    // 入力では時計が進まない。
    assert_eq!(rse_game_time(h), 0);
    assert_eq!(rse_world_free(h), 0);
}

// 置く・壊すの arg は、読まない所が 0 でなければ断り、何もしない。
#[test]
fn place_and_break_arguments_are_checked() {
    let h = rse_world_new();
    assert_eq!(load(h, &floor(), 0), 0);
    changes(h);
    let stone = id("stone");
    let place_arg = |arg: u32| rse_input(h, input_kind::PLACE, A, 1, 1, 1, arg);
    // 表の外の StateId・空気・上の 16 ビット。
    for arg in [
        STATE_COUNT as u32,
        0xFFFF,
        0,
        stone | 1 << 16,
        stone | 1 << 31,
        u32::MAX,
    ] {
        assert_eq!(place_arg(arg), status::ARGUMENT, "{arg:#x}");
    }
    // 壊すの arg は 0 だけ (石の床を壊そうとしても、断れば石は残る)。
    for arg in [1, stone, 1 << 16] {
        assert_eq!(
            rse_input(h, input_kind::BREAK, A, 0, 0, 0, arg),
            status::ARGUMENT,
            "{arg:#x}"
        );
    }
    // 区間の番号と種類の番号も断る (種類は 0〜3 だけ)。
    assert_eq!(
        rse_input(h, input_kind::PLACE, 4, 1, 1, 1, stone),
        status::ARGUMENT
    );
    assert_eq!(rse_input(h, 4, A, 1, 1, 1, stone), status::ARGUMENT);
    assert_eq!(rse_input(h, 4, A, 0, 0, 0, 0), status::ARGUMENT);
    assert_eq!(changes(h).unwrap(), []);
    // どの区間でも通る (区間の印は順を変えない)。
    for section in 0..4 {
        assert_eq!(
            rse_input(h, input_kind::PLACE, section, section as i32, 1, 1, stone),
            0
        );
    }
    assert_eq!(changes(h).unwrap().len(), 4);
    assert_eq!(rse_world_free(h), 0);
}

// 置く前に rse_load と同じ確かめ (can_set_all_raw) で、高さの外と格子の枠の外を断る (PR #16 の承認の 2)。
// rse-core を直に呼ぶと、高さの外は何もせず 0、格子の枠の外は書き込みまで進んで SAFETY になる。
// 窓口は OUT_OF_WORLD を返し、SAFETY を立てず、世界も格子も変えない。壊すは範囲を確かめず、同じ所でも 0。
#[test]
fn place_outside_the_world_is_refused_before_the_engine() {
    let h = rse_world_new();
    // 格子は (0, 0, 0) のセクションだけ。
    assert_eq!(load(h, &[cell(0, 0, 0, "stone")], 0), 0);
    changes(h);
    let mut direct = new_world(Config::default());
    assert!(direct.set_state_raw(Pos::new(0, 0, 0), StateId(id("stone") as u16)));
    let outside = [
        (0, 320, 0),
        (0, -65, 0),
        (0, i32::MAX, 0),
        (16 * 256, 1, 0),
        (0, 1, -16 * 256),
        (i32::MIN, 1, i32::MIN),
    ];
    for (x, y, z) in outside {
        for name in ["stone", "redstone_lamp"] {
            assert_eq!(place(h, x, y, z, name), status::OUT_OF_WORLD, "{x} {y} {z}");
        }
        assert_eq!(break_at(h, x, y, z), 0, "{x} {y} {z}");
        // 直に呼んだ時: 高さの外は 0、格子の枠の外は SAFETY。
        let state = StateId(id("stone") as u16);
        let pos = Pos::new(x, y, z);
        let raw = direct.input(InputSection::A, Input::Place { pos, state });
        assert!(!raw.accepted);
        let in_height = (-64..320).contains(&y);
        let expect = if in_height {
            Status::SAFETY
        } else {
            Status::OK
        };
        assert_eq!(raw.status, expect, "{x} {y} {z}");
    }
    assert_eq!(changes(h).unwrap(), []);
    // 断った置く入力は、状態コードを後に残さない。
    assert_eq!(rse_step(h, 1), 0);
    assert_eq!(changes(h).unwrap(), []);
    // 格子も広げていない: 枠の端 (255 セクション先) には置け、そこと合わせて枠を超える所は断る。
    assert_eq!(place(h, 16 * 255, 0, 0, "stone"), 0);
    assert_eq!(place(h, -1, 0, 0, "stone"), status::OUT_OF_WORLD);
    assert_eq!(changes(h).unwrap(), [cell(16 * 255, 0, 0, "stone")]);
    assert_eq!(rse_world_free(h), 0);
}

// 時計は L5 のゲーム時刻 (V01 §2.3・§4.4)。作った直後は 0、読み込みと入力では進まず、step(n) で n 進む。
// 世界ごとに別。ハンドルが無ければ -1。作業領域を書かず、広げない。
//
// B2 の時計 (Level::clock) は、凍結中の tick も含めて L5 といつも同じ数だけ進む (V01 §2.3 の表)。
// だから今は、取り違えても窓口からは見分けられない。分かれる素子が入ったら、ここで比べる。
#[test]
fn game_time_counts_ticks_only() {
    let (a, b) = (rse_world_new(), rse_world_new());
    assert_eq!((rse_game_time(a), rse_game_time(b)), (0, 0));
    assert_eq!(load(a, &circuit(), 0), 0);
    assert_eq!(rse_game_time(a), 0);
    assert_eq!(use_lever(a), 0);
    assert_eq!(place(a, 2, 1, 1, "redstone_wire"), 0);
    assert_eq!(break_at(a, 2, 1, 1), 0);
    assert_eq!(rse_game_time(a), 0);
    assert_eq!(rse_step(a, 3), 0);
    assert_eq!(rse_game_time(a), 3);
    assert_eq!(rse_step(a, 0), 0);
    assert_eq!(rse_game_time(a), 3);
    assert_eq!(rse_step(a, 5), 0);
    assert_eq!(rse_game_time(a), 8);
    // 世界ごとに別。
    assert_eq!(rse_game_time(b), 0);
    assert_eq!(rse_step(b, 1), 0);
    assert_eq!((rse_game_time(a), rse_game_time(b)), (8, 1));
    // 作業領域を書かず、広げない (結果は値で返す)。
    let base = rse_scratch(16);
    let size = registry::with(|r| r.scratch_len());
    poke(base, &[0xAB; 16]);
    assert_eq!(rse_game_time(a), 8);
    assert_eq!(rse_game_time(0), -1);
    assert_eq!(registry::with(|r| r.scratch_len()), size);
    assert_eq!(
        registry::with(|r| r.scratch_mut().bytes(base.addr(), 16).map(<[u8]>::to_vec)),
        Some(vec![0xAB; 16])
    );
    // 捨てると -1。残った世界は進み続ける。
    assert_eq!(rse_world_free(a), 0);
    assert_eq!(rse_game_time(a), -1);
    assert_eq!(rse_step(b, 2), 0);
    assert_eq!(rse_game_time(b), 3);
    assert_eq!(rse_world_free(b), 0);
    assert_eq!(rse_game_time(b), -1);
}

/// 試験の中の乱数 (線形合同。依存を足さない)。上の 32 ビットを使う。
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 32) % n as u64) as u32
    }
}

// 窓口を通した置く・壊す・使うを混ぜた列が、rse-engine を直に呼んだ結果と、入力ごとの状態コード・
// tick ごとの変化の列・ゲーム時刻まで同じになる。種と回数は固定。
#[test]
fn abi_matches_the_engine_with_place_and_break() {
    let palette = [
        "stone",
        "redstone_wire",
        WIRE_CROSS,
        "redstone_lamp",
        "redstone_lamp[lit=true]",
        "lever[face=floor,facing=north]",
        "lever[face=floor,facing=east]",
        "lever[face=wall,facing=south]",
        "lever[face=ceiling,facing=west]",
        // 規則の無い要求 (UNIMPLEMENTED)。
        "lever[face=floor,facing=north,powered=true]",
    ]
    .map(id);
    let sections = [
        InputSection::B5,
        InputSection::C,
        InputSection::A,
        InputSection::B1,
    ];
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
    let h = rse_world_new();
    let mut direct = new_world(Config::default());
    let board: Vec<CellRec> = (0..6)
        .flat_map(|x| (0..6).map(move |z| cell(x, 0, z, "stone")))
        .collect();
    assert_eq!(load(h, &board, 0), 0);
    for c in &board {
        assert!(direct.set_state_raw(Pos::new(c.x, c.y, c.z), StateId(c.state as u16)));
    }
    assert_eq!(changes(h).unwrap(), drain(&mut direct));
    let mut rng = Lcg(0x5EED_0013);
    let (mut placed, mut unimplemented) = (0, 0);
    for t in 1..=400 {
        assert_eq!(rse_step(h, 1), direct.tick().0, "tick {t}");
        for _ in 0..rng.below(4) {
            let (x, y, z) = (
                rng.below(6) as i32,
                rng.below(3) as i32,
                rng.below(6) as i32,
            );
            let pos = Pos::new(x, y, z);
            let section = rng.below(4);
            let (kind, arg, input) = match rng.below(4) {
                0 | 1 => {
                    let state = palette[rng.below(palette.len() as u32) as usize];
                    let input = Input::Place {
                        pos,
                        state: StateId(state as u16),
                    };
                    (input_kind::PLACE, state, input)
                }
                2 => (input_kind::BREAK, 0, Input::Break { pos }),
                _ => (input_kind::USE, 0, Input::Use { pos }),
            };
            let got = rse_input(h, kind, section, x, y, z, arg);
            let want = direct.input(sections[section as usize], input);
            assert_eq!(got, want.status.0, "tick {t} {input:?}");
            placed += usize::from(kind == input_kind::PLACE && want.accepted);
            unimplemented += usize::from(want.status == Status::UNIMPLEMENTED);
        }
        assert_eq!(changes(h).unwrap(), drain(&mut direct), "tick {t}");
        assert_eq!(rse_game_time(h), direct.game_time(), "tick {t}");
    }
    assert_eq!(rse_game_time(h), 400);
    // 列が素通りしていない (置けた入力も、規則の無い要求も、どちらも通った)。
    assert!(placed > 50, "{placed}");
    assert!(unimplemented > 5, "{unimplemented}");
    assert_eq!(rse_world_free(h), 0);
}
