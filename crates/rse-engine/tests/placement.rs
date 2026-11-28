//! 置く・壊すの入力を、配る版の振り分けを通して確かめる。
//!
//! 手順 (置き換え可能 → 置いた時の状態 → 支え → フラグ 11) は rse-core の input にあり、呼ぶ順は rse-core の
//! 試験が固めている。ここは、族の関数と組んだ時の盤面を見る。
//!
//! 族の素子を使うので、feature (redstone) を外した組み立てでは空になる。外した時の答えは src/lib.rs の試験にある。
#![cfg(feature = "redstone")]

use rse_engine::{Config, Input, InputOutcome, InputSection, Pos, StateId, Status, World};

const LEVER_OFF: &str = "lever[face=floor,facing=north,powered=false]";

fn parse(text: &str) -> StateId {
    StateId::parse(text).unwrap()
}

/// 石の床 (y=0) を敷いた世界。x と z は 0 から `size - 1` まで。
fn floored(size: i32) -> World {
    let mut world = rse_engine::new_world(Config::default());
    for x in 0..size {
        for z in 0..size {
            assert!(world.set_state_raw(Pos::new(x, 0, z), parse("stone")));
        }
    }
    world.drain_changes();
    world
}

fn place(world: &mut World, pos: Pos, text: &str) -> InputOutcome {
    let state = parse(text);
    world.input(InputSection::A, Input::Place { pos, state })
}

/// 置けて、状態コードも立たないこと。
fn placed(world: &mut World, pos: Pos, text: &str) {
    let out = place(world, pos, text);
    assert!(out.accepted, "{text} を {pos:?} に置けない");
    assert!(out.status.is_ok(), "{text} {pos:?}: {:?}", out.status);
}

fn broke(world: &mut World, pos: Pos) {
    let out = world.input(InputSection::A, Input::Break { pos });
    assert!(out.accepted, "{pos:?} を壊せない");
    assert!(out.status.is_ok(), "{pos:?}: {:?}", out.status);
}

fn at(world: &World, pos: Pos) -> String {
    world.get_state(pos).to_string()
}

/// ダストの文字列。接続は、北・東・南・西の順の真偽 (真なら side)。
fn wire(north: bool, east: bool, south: bool, west: bool, power: u8) -> String {
    let side = |on| if on { "side" } else { "none" };
    format!(
        "redstone_wire[east={},north={},power={power},south={},west={}]",
        side(east),
        side(north),
        side(south),
        side(west)
    )
}

const REFUSED: InputOutcome = InputOutcome {
    accepted: false,
    status: Status::OK,
};

// 孤立したダストは十字になり、接続が 1 本なら直線になる (V06 §4.3 の表・§4.4)。
// 要求の状態のプロパティは読まない。点を要求しても十字になる (/setblock なら点になる所。V06 §4.14.1)。
#[test]
fn placed_wire_takes_its_shape_from_the_surroundings() {
    let mut world = floored(5);
    let a = Pos::new(2, 1, 2);
    placed(&mut world, a, "redstone_wire");
    assert_eq!(at(&world, a), wire(true, true, true, true, 0));
    // 既定の状態 (4 方向 none) と違う形を要求しても、周りで決まる。
    let far = Pos::new(0, 1, 0);
    placed(
        &mut world,
        far,
        "redstone_wire[east=up,north=none,power=7,south=none,west=none]",
    );
    assert_eq!(at(&world, far), wire(true, true, true, true, 0));
    // 東の隣に置くと、新しいダストは西への 1 本で、東西の直線になる。先に居たダストも直線に直る (V06 §4.6)。
    let b = Pos::new(3, 1, 2);
    placed(&mut world, b, "redstone_wire");
    assert_eq!(at(&world, b), wire(false, true, false, true, 0));
    assert_eq!(at(&world, a), wire(false, true, false, true, 0));
    // レバー (信号源) の隣でも 1 本になる (V06 §4.1)。
    let lever = Pos::new(0, 1, 4);
    placed(&mut world, lever, LEVER_OFF);
    let c = Pos::new(0, 1, 3);
    placed(&mut world, c, "redstone_wire");
    assert_eq!(at(&world, c), wire(true, false, true, false, 0));
    assert_eq!(world.scheduled_tick_count(), 0);
}

// 隣を置く・壊すたびに、居るダストの形が移る (V06 §4.3・§4.6)。
// 十字 → 直線 (隣 1 本) → 角 (隣 2 本) → 直線 (1 本を壊す) → 十字 (孤立に戻る)。
#[test]
fn shape_follows_neighbors_as_they_come_and_go() {
    let mut world = floored(5);
    let a = Pos::new(2, 1, 2);
    let east = Pos::new(3, 1, 2);
    let north = Pos::new(2, 1, 1);
    placed(&mut world, a, "redstone_wire");
    assert_eq!(at(&world, a), wire(true, true, true, true, 0));
    placed(&mut world, east, "redstone_wire");
    assert_eq!(at(&world, a), wire(false, true, false, true, 0));
    placed(&mut world, north, "redstone_wire");
    assert_eq!(at(&world, a), wire(true, true, false, false, 0));
    // 北のダストは、南への 1 本で南北の直線。
    assert_eq!(at(&world, north), wire(true, false, true, false, 0));
    broke(&mut world, east);
    assert_eq!(at(&world, east), "air");
    assert_eq!(at(&world, a), wire(true, false, true, false, 0));
    broke(&mut world, north);
    assert_eq!(at(&world, a), wire(true, true, true, true, 0));
    // ランプと石は接続先でない。隣に置いても形は変わらない (V06 §4.1)。
    placed(&mut world, east, "redstone_lamp");
    placed(&mut world, north, "stone");
    assert_eq!(at(&world, a), wire(true, true, true, true, 0));
    assert_eq!(world.scheduled_tick_count(), 0);
    assert_eq!(world.game_time(), 0);
}

// 横からの形状更新は、保持値が十字でなく、その方向の接続の有無が変わらなければ、その方向だけを直す (V06 §4.6 の手順 2)。
// 他の 3 方向の保持値には触れないので、実情とずれた保持値はそのまま残る。全部を計算し直す形 (V06 §4.3) なら、
// 孤立したダストは十字になってしまう。
// - 孤立なのに東西の直線を保持したダストを更新なしで置き、北にランプ・南に石・西にダストを置く。
//   北と南は none のまま、西は side のままなので、東西の直線が残る (孤立の間も十字にならない)。
// - 対になる形: 南北の直線を保持した孤立のダストの西にダストを置くと、西の接続の有無が変わるので手順 3 になり、
//   全部を計算し直す。生の接続は西の 1 本なので、東西の直線になる (保持していた南北は消える)。
#[test]
fn side_shape_update_fixes_only_that_direction() {
    let mut world = floored(5);
    let a = Pos::new(2, 1, 2);
    let line_x = wire(false, true, false, true, 0);
    assert!(world.set_state_raw(a, parse(&line_x)));
    world.drain_changes();
    placed(&mut world, Pos::new(2, 1, 1), "redstone_lamp");
    assert_eq!(at(&world, a), line_x);
    placed(&mut world, Pos::new(2, 1, 3), "stone");
    assert_eq!(at(&world, a), line_x);
    placed(&mut world, Pos::new(1, 1, 2), "redstone_wire");
    assert_eq!(at(&world, a), line_x);

    let b = Pos::new(4, 1, 0);
    assert!(world.set_state_raw(b, parse(&wire(true, false, true, false, 0))));
    placed(&mut world, Pos::new(3, 1, 0), "redstone_wire");
    assert_eq!(at(&world, b), line_x);
    assert_eq!(world.scheduled_tick_count(), 0);
}

// 支えの無い所と、空気でない所には置けない。断った時は何も変わらず、状態コードも立たない。
#[test]
fn refuses_without_support_or_on_a_block() {
    let mut world = floored(3);
    let spot = Pos::new(1, 1, 1);
    // 支え: ダストは真下の上面 (V06 §4.5)、床置きのレバーは真下の上面 (V09 §4.1)。宙には置けない。
    let air_below = Pos::new(1, 3, 1);
    assert_eq!(place(&mut world, air_below, "redstone_wire"), REFUSED);
    assert_eq!(place(&mut world, air_below, LEVER_OFF), REFUSED);
    // ダストとレバーの上は支えにならない (V05 §4.7.5)。
    placed(&mut world, spot, "redstone_wire");
    let above = Pos::new(1, 2, 1);
    assert_eq!(place(&mut world, above, "redstone_wire"), REFUSED);
    assert_eq!(place(&mut world, above, LEVER_OFF), REFUSED);
    // ランプと石は支えが要らない。宙にも置ける。
    placed(&mut world, air_below, "redstone_lamp");
    placed(&mut world, Pos::new(0, 3, 0), "stone");
    // ランプの上は支えになる。
    placed(&mut world, Pos::new(1, 4, 1), "redstone_wire");
    // 空気でない所には、何も置けない (V09 付録 A.3)。ダストも石も置き換え可能でない。
    let before = at(&world, spot);
    for text in ["redstone_wire", "redstone_lamp", "stone", LEVER_OFF] {
        assert_eq!(place(&mut world, spot, text), REFUSED, "{text}");
        assert_eq!(
            place(&mut world, Pos::new(1, 0, 1), text),
            REFUSED,
            "{text}"
        );
    }
    assert_eq!(at(&world, spot), before);
    assert_eq!(at(&world, Pos::new(1, 0, 1)), "stone");
    // 空気は置く物ではない。
    assert_eq!(place(&mut world, Pos::new(2, 1, 2), "air"), REFUSED);
    assert_eq!(at(&world, Pos::new(2, 1, 2)), "air");
}

// レバーは、入った状態を要求しても切れて置かれる。受電を読まないので、強充電された支えに付けても、
// 入ったレバー (信号源) の隣に置いても同じである (V09 §4.1)。予約を積まず、置いたマスのほかは何も変わらない。
#[test]
fn powered_lever_requests_are_placed_off() {
    let mut world = floored(5);
    // 入ったレバー A (床置き) が、真下の石を強充電する。
    let a = Pos::new(1, 1, 1);
    placed(&mut world, a, LEVER_OFF);
    assert!(world.input(InputSection::A, Input::Use { pos: a }).accepted);
    assert_eq!(world.direct_signal_to(Pos::new(1, 0, 1)), 15);
    world.drain_changes();
    for (pos, request, expect) in [
        // 強充電された石の下面に、天井付けで付ける。
        (
            Pos::new(1, -1, 1),
            "lever[face=ceiling,facing=west,powered=true]",
            "lever[face=ceiling,facing=west,powered=false]",
        ),
        // 入ったレバーの東隣に、床置きで置く。
        (
            Pos::new(2, 1, 1),
            "lever[face=floor,facing=south,powered=true]",
            "lever[face=floor,facing=south,powered=false]",
        ),
    ] {
        placed(&mut world, pos, request);
        assert_eq!(at(&world, pos), expect);
        let changed: Vec<(Pos, String)> = world
            .drain_changes()
            .iter()
            .map(|c| (c.pos, c.state.to_string()))
            .collect();
        assert_eq!(changed, [(pos, expect.to_string())], "{request}");
        assert_eq!(world.scheduled_tick_count(), 0, "{request}");
    }
    assert_eq!(at(&world, a), "lever[face=floor,facing=north,powered=true]");
}

// 受電した所に置いたランプは、点いた状態で置かれる。予約は積まない (V11 §4.1)。
// 入ったレバーの横に置いたダストは、設置フックでその場で給電される (V06 §4.14)。
#[test]
fn placing_next_to_power_takes_it_at_once() {
    let mut world = floored(5);
    let lever = Pos::new(2, 1, 2);
    placed(&mut world, lever, LEVER_OFF);
    // 切れている間に置いたランプは消えている。点いた状態を要求しても、要求の lit は読まずに消えて置かれ、
    // 消えるための予約も積まない (V11 §4.1。「どの状態を渡しても同じ」の約束)。
    let west = Pos::new(1, 1, 2);
    placed(&mut world, west, "redstone_lamp[lit=true]");
    assert_eq!(at(&world, west), "redstone_lamp[lit=false]");
    assert_eq!(world.scheduled_tick_count(), 0);
    let far = Pos::new(4, 1, 4);
    placed(&mut world, far, "redstone_lamp[lit=true]");
    assert_eq!(at(&world, far), "redstone_lamp[lit=false]");
    assert_eq!(world.scheduled_tick_count(), 0);
    assert!(
        world
            .input(InputSection::A, Input::Use { pos: lever })
            .accepted
    );
    assert_eq!(at(&world, west), "redstone_lamp[lit=true]");
    let east = Pos::new(3, 1, 2);
    placed(&mut world, east, "redstone_lamp[lit=false]");
    assert_eq!(at(&world, east), "redstone_lamp[lit=true]");
    assert_eq!(world.scheduled_tick_count(), 0);
    // ダスト: 南に置くと、北 (レバー) への 1 本で直線になり、power は 15。その先に置くと 14。
    let south = Pos::new(2, 1, 3);
    placed(&mut world, south, "redstone_wire");
    assert_eq!(at(&world, south), wire(true, false, true, false, 15));
    let further = Pos::new(2, 1, 4);
    placed(&mut world, further, "redstone_wire");
    assert_eq!(at(&world, further), wire(true, false, true, false, 14));
    assert_eq!(world.scheduled_tick_count(), 0);
    assert_eq!(world.game_time(), 0);
}

// 壊すと、撤去フックが書き込みの中で走る。
// - 間のダストを壊すと、先のダストはその場で 0 に戻る (V06 §4.15)。
// - 入ったレバーを壊すと、隣のダストは 0 に戻り、隣のランプは 4gt 後に消える (V09 §4.1・V11 §4.1)。
// - 真下の石を壊すと、上のダストは支えを失って消える (V06 §4.5)。
#[test]
fn breaking_runs_the_removal_hooks() {
    let mut world = floored(6);
    let lever = Pos::new(0, 1, 0);
    let wires = [Pos::new(1, 1, 0), Pos::new(2, 1, 0), Pos::new(3, 1, 0)];
    let lamp = Pos::new(0, 1, 1);
    placed(&mut world, lever, LEVER_OFF);
    for pos in wires {
        placed(&mut world, pos, "redstone_wire");
    }
    placed(&mut world, lamp, "redstone_lamp");
    world.input(InputSection::A, Input::Use { pos: lever });
    assert_eq!(at(&world, wires[2]), wire(false, true, false, true, 13));
    assert_eq!(at(&world, lamp), "redstone_lamp[lit=true]");

    broke(&mut world, wires[1]);
    assert_eq!(at(&world, wires[0]), wire(false, true, false, true, 15));
    assert_eq!(at(&world, wires[2]), wire(true, true, true, true, 0));
    placed(&mut world, wires[1], "redstone_wire");
    assert_eq!(at(&world, wires[1]), wire(false, true, false, true, 14));
    assert_eq!(at(&world, wires[2]), wire(false, true, false, true, 13));

    broke(&mut world, lever);
    assert_eq!(at(&world, lever), "air");
    assert_eq!(at(&world, wires[0]), wire(false, true, false, true, 0));
    assert_eq!(at(&world, wires[2]), wire(false, true, false, true, 0));
    assert_eq!(at(&world, lamp), "redstone_lamp[lit=true]");
    assert_eq!(world.scheduled_tick_count(), 1);
    for _ in 0..3 {
        assert!(world.tick().is_ok());
        assert_eq!(at(&world, lamp), "redstone_lamp[lit=true]");
    }
    assert!(world.tick().is_ok());
    assert_eq!(at(&world, lamp), "redstone_lamp[lit=false]");

    broke(&mut world, Pos::new(3, 0, 0));
    assert_eq!(at(&world, wires[2]), "air");
    assert_eq!(at(&world, wires[1]), wire(false, true, false, true, 0));
    // 空気を壊しても何も起きない。
    let out = world.input(InputSection::A, Input::Break { pos: wires[2] });
    assert_eq!(out, REFUSED);
}

// ---- 性質の試験 ----
//
// 主張: 次の範囲の盤面では、置く・壊すをどんな順で何回入れても、最後の盤面は「どのマスに何があるか」だけで決まる。
// ダストの形は横 4 マスの種類 (ダストかレバーなら接続) と V06 §4.3 の規則で決まり、置いた順にも壊した履歴にも依らない。
//
// 範囲 (これより広い盤面には主張しない):
// - 素子は 1 層 (y=1) だけ。真下 (y=0) は全部石で、真上 (y=2) は空気。段差が無いので、接続は side か none だけ。
// - 素子はダスト・床置きのレバー (facing は 4 つのどれか・切れている)・ランプの 3 つ。
// - 全部切れている。「使う」を送らない (レバーは入らず、ダストは点にならない)。
//
// メモ (V06 §2.1・§4.2〜§4.4・§4.6・§4.7 と V04 §4.1) から導いた性質で、実機では確かめていない。
// 給電中の盤面には言えない。入ったレバーの横に置くと、設置フックで power が変わって外側の SU が出ない (V06 §4.14 の
// 落とし穴) ので、周りへ届く更新が変わり、置く順で形が変わることがある (この repo に入れる前の下調べで見えた)。

/// 盤面の 1 辺。
const SIZE: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Piece {
    Wire,
    /// 床置きのレバー。値は facing の添字。
    Lever(usize),
    Lamp,
}

const FACINGS: [&str; 4] = ["north", "south", "west", "east"];

impl Piece {
    /// 置く入力で運ぶ状態。
    fn request(self) -> String {
        match self {
            Piece::Wire => "redstone_wire".into(),
            Piece::Lever(facing) => {
                format!("lever[face=floor,facing={},powered=false]", FACINGS[facing])
            }
            Piece::Lamp => "redstone_lamp".into(),
        }
    }
}

/// 試験の中だけの乱数 (線形合同)。種は固定で、依存を足さない。
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// 盤面の写し。エンジンとは別に、置けたかどうかだけで更新する。
struct Model([[Option<Piece>; SIZE as usize]; SIZE as usize]);

impl Model {
    fn get(&self, x: i32, z: i32) -> Option<Piece> {
        if (0..SIZE).contains(&x) && (0..SIZE).contains(&z) {
            self.0[z as usize][x as usize]
        } else {
            None
        }
    }

    /// 横の隣が、ダストの接続先か (V06 §4.1: ダストと信号源)。
    fn connects(&self, x: i32, z: i32) -> bool {
        matches!(self.get(x, z), Some(Piece::Wire | Piece::Lever(_)))
    }

    /// そのマスの期待の文字列。種類の配置だけから作る。
    fn expect(&self, x: i32, z: i32) -> String {
        match self.get(x, z) {
            None => "air".into(),
            Some(Piece::Wire) => {
                // 生の接続 (北は z-1・東は x+1)。
                let (n, e) = (self.connects(x, z - 1), self.connects(x + 1, z));
                let (s, w) = (self.connects(x, z + 1), self.connects(x - 1, z));
                // 延長 (V06 §4.3 の手順 3): 南北が空なら東西を、東西が空なら南北を side にする。
                // 「空か」は延長の前の値で決める。0 本なら十字、1 本なら直線、2 本以上はそのまま。
                let (ns_empty, ew_empty) = (!n && !s, !e && !w);
                wire(
                    n || ew_empty,
                    e || ns_empty,
                    s || ew_empty,
                    w || ns_empty,
                    0,
                )
            }
            Some(piece @ Piece::Lever(_)) => piece.request(),
            Some(Piece::Lamp) => "redstone_lamp[lit=false]".into(),
        }
    }
}

/// 置く・壊すを `steps` 回、乱数の順で入れる。置けたか・壊せたかも写しと比べる。
fn run_history(rng: &mut Lcg, steps: usize) -> (World, Model) {
    let mut world = floored(SIZE);
    let mut model = Model([[None; SIZE as usize]; SIZE as usize]);
    for step in 0..steps {
        let (x, z) = (rng.below(SIZE as u64) as i32, rng.below(SIZE as u64) as i32);
        let pos = Pos::new(x, 1, z);
        let cell = &mut model.0[z as usize][x as usize];
        // 4 回に 1 回は壊す。残りは置く (ダストを多めにして、形の移りを増やす)。
        let (out, expect_accepted) = if rng.below(4) == 0 {
            let had = cell.take().is_some();
            (world.input(InputSection::A, Input::Break { pos }), had)
        } else {
            let piece = match rng.below(8) {
                0..=4 => Piece::Wire,
                5 => Piece::Lamp,
                _ => Piece::Lever(rng.below(4) as usize),
            };
            let free = cell.is_none();
            if free {
                *cell = Some(piece);
            }
            (place(&mut world, pos, &piece.request()), free)
        };
        assert_eq!(
            out,
            InputOutcome {
                accepted: expect_accepted,
                status: Status::OK
            },
            "{step} 手目 {pos:?}"
        );
    }
    (world, model)
}

/// 世界が、写しから作った期待と文字列まで同じで、静かであること。
fn assert_settled_as(world: &mut World, model: &Model, case: &str) {
    for x in 0..SIZE {
        for z in 0..SIZE {
            assert_eq!(
                at(world, Pos::new(x, 1, z)),
                model.expect(x, z),
                "{case} ({x}, 1, {z})"
            );
            assert_eq!(at(world, Pos::new(x, 0, z)), "stone", "{case}");
            assert_eq!(at(world, Pos::new(x, 2, z)), "air", "{case}");
        }
    }
    assert_eq!(world.scheduled_tick_count(), 0, "{case}");
    assert_eq!(world.game_time(), 0, "{case}");
    // 8 tick 回しても、何も変わらない (19 §4.2 の「点検」と同じ)。
    world.drain_changes();
    for _ in 0..8 {
        assert!(world.tick().is_ok(), "{case}");
    }
    assert!(world.drain_changes().is_empty(), "{case}");
    assert_eq!(world.game_time(), 8, "{case}");
}

// 乱数の履歴 2000 本 × 48 手。種は固定。
#[test]
fn final_board_depends_only_on_what_is_where() {
    let mut rng = Lcg(0x5EED_0013);
    for case in 0..2000 {
        let (mut world, model) = run_history(&mut rng, 48);
        assert_settled_as(&mut world, &model, &format!("履歴 {case}"));
    }
}

// 同じ配置を、違う順で組む: 乱数の履歴の最後の盤面を、何も無い世界へ行の順と逆の順で置き直す。
// 3 つの世界が、マスごとに同じ状態になる。
#[test]
fn same_layout_in_any_order_gives_the_same_board() {
    let mut rng = Lcg(0x0D0E_5713);
    for case in 0..300 {
        let (world, model) = run_history(&mut rng, 48);
        let cells: Vec<(i32, i32)> = (0..SIZE)
            .flat_map(|z| (0..SIZE).map(move |x| (x, z)))
            .collect();
        let forward = cells.clone();
        let backward: Vec<(i32, i32)> = cells.iter().rev().copied().collect();
        for order in [forward, backward] {
            let mut rebuilt = floored(SIZE);
            for (x, z) in order {
                if let Some(piece) = model.get(x, z) {
                    placed(&mut rebuilt, Pos::new(x, 1, z), &piece.request());
                }
            }
            for (x, z) in &cells {
                let pos = Pos::new(*x, 1, *z);
                assert_eq!(
                    rebuilt.get_state(pos),
                    world.get_state(pos),
                    "履歴 {case} {pos:?}"
                );
            }
        }
    }
}
