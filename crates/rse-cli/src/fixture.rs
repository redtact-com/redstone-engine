//! fixture の実行器 (19 §4.1・docs/architecture.md §6)。
//!
//! fixture は、実機で撮った「初めの盤面・入力・tick ごとの変化」である (conformance/fixtures/SOURCES.md)。
//! 実行器は盤面をそのまま置き、毎 tick、領域の全ブロックの blockstate の文字列を期待と比べる。
//!
//! - 規約は「state[t] は tick t の全相の後、tick t の入力の直後」
//!   (redstone-sim の tools/mc-harness/README.md の「tick 規約」・19 §4.1)。t は 0 から `ticks` まで。
//! - **初めの盤面は tick −1 の状態である**。t = 0 では tick を進めず、tick 0 の入力だけを適用して読む。
//!   tick を進めるのは t ≧ 1 だけ。26.3 のオラクル (redstone-sim の tools/gt-oracle の Driver。
//!   settle の最後に tick −1 を撮り、同じ tick の中で tick 0 の入力を入れて tick 0 を撮る) と、
//!   redstone-sim の TS の実行器 (packages/sim/src/fixture-driver.ts の runFixtureOnSim) と同じ数え方。
//!   t = 0 の前に 1 回進めると、予約を持ち込む fixture で 1 tick ずれる。
//! - 盤面は更新を出さずに置く (19 §4.2 の「そのまま」の型)。
//! - 期待は「変化のあった tick の差分」で書いてある。差分を積んだ盤面と、領域の全マスを比べる。
//!   書いていないマスは空気である。
//! - 期待の文字列は、書いてあるまま比べる。エンジンに合わせて直さない。
//! - 状態コードが立ったら不一致と同じに扱う (未実装の経路を黙って通らない)。
//! - 表に無いブロック・知らない入力は、エラーで止める (19 §11 の 5)。
//! - **読まない欄があれば、エラーで止める**。redstone-sim の fixture には、ここがまだ読まない欄
//!   (trustAuthored・scheduled・comparators・cooldowns・skipUntil・skipReason・blocks の items ほか) を持つ物がある。
//!   黙って読み飛ばすと、違う比べ方のまま「一致」と出てしまう。

use rse_engine::rse_core::input::{MarkedInput, sort_by_section};
use rse_engine::{Config, Input, InputSection, Pos, StateId, Status, new_world};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// 位置の鍵 (y・z・x の順に並ぶ)。
type Key = (i32, i32, i32);

fn key(pos: Pos) -> Key {
    (pos.y, pos.z, pos.x)
}

/// 読み込んだ fixture。
#[derive(Clone, Debug)]
pub struct Fixture {
    pub name: String,
    /// 撮った版 (例: "1.21.1")。入力の並べ方に使う。
    pub mc_version: String,
    /// 最後の tick。0 からこの値までを比べる。
    pub ticks: u32,
    /// 比べる領域の角 (両端を含む。成分ごとに from ≦ to)。
    pub from: Pos,
    pub to: Pos,
    /// 初めの盤面。
    pub blocks: Vec<(Pos, StateId)>,
    /// 入力。fixture に書いてある並びのまま持つ。
    pub inputs: Vec<(u32, MarkedInput)>,
    /// 期待。tick ごとの「位置と blockstate の文字列」。
    pub expect: Vec<(u32, Vec<(Pos, String)>)>,
}

/// 1 マスの不一致。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mismatch {
    pub tick: u32,
    pub pos: Pos,
    pub expect: String,
    pub actual: String,
}

/// 実行の結果。
#[derive(Clone, Debug)]
pub struct Report {
    pub name: String,
    pub ticks: u32,
    /// tick を進めた回数。t = 0 では進めないので、`ticks` と同じになる。
    pub ticked: u32,
    /// 比べたマスの数 (tick ごとの領域の全マスの合計)。
    pub compared: usize,
    pub mismatches: Vec<Mismatch>,
    /// 状態コードが立った tick と、その値。
    pub statuses: Vec<(u32, Status)>,
}

impl Report {
    /// 毎 tick の全マスが一致し、状態コードも立たなかったか。
    pub fn passed(&self) -> bool {
        self.mismatches.is_empty() && self.statuses.is_empty()
    }
}

/// 合わない tick と座標が分かる形で書く。
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.passed() {
            return write!(
                f,
                "{}: 一致 (tick 0〜{}・{} マス)",
                self.name, self.ticks, self.compared
            );
        }
        write!(
            f,
            "{}: 不一致 {} マス・状態コード {} 件",
            self.name,
            self.mismatches.len(),
            self.statuses.len()
        )?;
        for m in &self.mismatches {
            write!(
                f,
                "\n  tick {} ({}, {}, {}): 期待 {} / 実際 {}",
                m.tick, m.pos.x, m.pos.y, m.pos.z, m.expect, m.actual
            )?;
        }
        for (tick, status) in &self.statuses {
            let names: Vec<&str> = Status::ALL
                .iter()
                .filter(|(bit, _)| status.contains(*bit))
                .map(|(_, name)| *name)
                .collect();
            write!(f, "\n  tick {tick}: 状態コード {}", names.join(" | "))?;
        }
        Ok(())
    }
}

/// 今読む欄 (上の段)。description と generated は中身を使わないが、比べ方を変えない欄なので通す。
const ROOT_KEYS: &[&str] = &[
    "name",
    "description",
    "mcVersion",
    "ticks",
    "region",
    "blocks",
    "inputs",
    "expect",
    "generated",
];
const REGION_KEYS: &[&str] = &["from", "to"];
const BLOCK_KEYS: &[&str] = &["pos", "block"];
/// 入力の項目 (action が use の時)。
const USE_INPUT_KEYS: &[&str] = &["tick", "pos", "action"];
const EXPECT_KEYS: &[&str] = &["tick", "changes"];
const CHANGE_KEYS: &[&str] = &["pos", "block"];

/// 読まない欄があれば断る。`place` は、エラーに出す場所の名前。
fn only_known(value: &Value, place: &str, known: &[&str]) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or(format!("{place} がオブジェクトでない"))?;
    let extra: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| !known.contains(k))
        .collect();
    if extra.is_empty() {
        Ok(())
    } else {
        Err(format!("{place} に読まない欄がある: {}", extra.join(", ")))
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value, String> {
    value.get(name).ok_or(format!("欄 {name} が無い"))
}

fn text(value: &Value, name: &str) -> Result<String, String> {
    field(value, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or(format!("欄 {name} が文字列でない"))
}

fn list<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, String> {
    field(value, name)?
        .as_array()
        .ok_or(format!("欄 {name} が配列でない"))
}

fn tick_of(value: &Value, name: &str) -> Result<u32, String> {
    field(value, name)?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(format!("欄 {name} が 0 以上の整数でない"))
}

fn pos_of(value: &Value, name: &str) -> Result<Pos, String> {
    let bad = || format!("欄 {name} が整数 3 つの配列でない");
    let parts = field(value, name)?.as_array().ok_or_else(bad)?;
    let coords: Vec<i32> = parts
        .iter()
        .filter_map(|n| n.as_i64().and_then(|n| i32::try_from(n).ok()))
        .collect();
    match (parts.len(), coords.as_slice()) {
        (3, [x, y, z]) => Ok(Pos::new(*x, *y, *z)),
        _ => Err(bad()),
    }
}

impl Fixture {
    /// fixture の JSON を読む。
    pub fn parse(json: &str) -> Result<Fixture, String> {
        let root: Value =
            serde_json::from_str(json).map_err(|e| format!("JSON が読めない: {e}"))?;
        only_known(&root, "fixture", ROOT_KEYS)?;
        let region = field(&root, "region")?;
        only_known(region, "region", REGION_KEYS)?;
        let (a, b) = (pos_of(region, "from")?, pos_of(region, "to")?);

        // 表に無いブロックは、全部集めてから断る (19 §3.1 の「未知」の段)。
        let mut blocks = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for entry in list(&root, "blocks")? {
            only_known(entry, "blocks の項目", BLOCK_KEYS)?;
            let name = text(entry, "block")?;
            match StateId::parse(&name) {
                Ok(state) => blocks.push((pos_of(entry, "pos")?, state)),
                Err(_) => {
                    if !unknown.contains(&name) {
                        unknown.push(name);
                    }
                }
            }
        }
        if !unknown.is_empty() {
            return Err(format!("状態表に無いブロック: {}", unknown.join(", ")));
        }

        let mut inputs = Vec::new();
        for entry in list(&root, "inputs")? {
            let pos = pos_of(entry, "pos")?;
            let marked = match text(entry, "action")?.as_str() {
                // 右クリックはプレイヤーの操作 (区間 A。19 §4.1)。
                "use" => {
                    only_known(entry, "inputs の項目 (use)", USE_INPUT_KEYS)?;
                    MarkedInput {
                        section: InputSection::A,
                        input: Input::Use { pos },
                    }
                }
                other => return Err(format!("対応していない入力: {other}")),
            };
            inputs.push((tick_of(entry, "tick")?, marked));
        }

        let mut expect = Vec::new();
        for entry in list(&root, "expect")? {
            only_known(entry, "expect の項目", EXPECT_KEYS)?;
            let mut changes = Vec::new();
            for change in list(entry, "changes")? {
                only_known(change, "changes の項目", CHANGE_KEYS)?;
                changes.push((pos_of(change, "pos")?, text(change, "block")?));
            }
            expect.push((tick_of(entry, "tick")?, changes));
        }

        Ok(Fixture {
            name: text(&root, "name")?,
            mc_version: text(&root, "mcVersion")?,
            ticks: tick_of(&root, "ticks")?,
            from: Pos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            to: Pos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
            blocks,
            inputs,
            expect,
        })
    }

    /// tick `t` の入力を、適用する順で返す。
    ///
    /// 26.3 の読みでは、同じ境界の入力はコマンド (C) → プレイヤーの操作 (A) の順である。
    /// ただし 1.21.1 で撮った fixture は到着順で適用されていたので、26.3 で撮り直すまでは
    /// 書いてある並びのまま適用する (V01 §2.4 の「同じ境界の中の C と A の前後」)。
    fn inputs_at(&self, t: u32) -> Vec<MarkedInput> {
        let mut now: Vec<MarkedInput> = self
            .inputs
            .iter()
            .filter(|(tick, _)| *tick == t)
            .map(|(_, input)| *input)
            .collect();
        if !self.mc_version.starts_with("1.21") {
            sort_by_section(&mut now);
        }
        now
    }

    /// 領域の全マス。
    fn region(&self) -> impl Iterator<Item = Pos> + '_ {
        (self.from.y..=self.to.y).flat_map(move |y| {
            (self.from.z..=self.to.z)
                .flat_map(move |z| (self.from.x..=self.to.x).map(move |x| Pos::new(x, y, z)))
        })
    }

    /// 回して、毎 tick の領域の全マスを期待と比べる。
    pub fn run(&self) -> Result<Report, String> {
        let mut world = new_world(Config::default());
        // 期待の盤面。領域の全マスを空気で埋め、初めの盤面を載せる。
        let mut board: BTreeMap<Key, String> = self
            .region()
            .map(|pos| (key(pos), StateId::AIR.to_string()))
            .collect();
        for (pos, state) in &self.blocks {
            if !world.set_state_raw(*pos, *state) {
                return Err(format!(
                    "({}, {}, {}) に置けない (高さか格子の枠の外)",
                    pos.x, pos.y, pos.z
                ));
            }
            // 領域の外のブロックは置くだけで、比べない。
            let shown = state.to_string();
            if let Some(cell) = board.get_mut(&key(*pos)) {
                *cell = shown;
            }
        }

        let mut report = Report {
            name: self.name.clone(),
            ticks: self.ticks,
            ticked: 0,
            compared: 0,
            mismatches: Vec::new(),
            statuses: Vec::new(),
        };
        for t in 0..=self.ticks {
            // 初めの盤面は tick −1 の状態。t = 0 では進めず、tick 0 の入力だけを適用する (冒頭の注)。
            let mut status = Status::OK;
            if t > 0 {
                status |= world.tick();
                report.ticked += 1;
            }
            for marked in self.inputs_at(t) {
                status |= world.input(marked.section, marked.input).status;
            }
            if !status.is_ok() {
                report.statuses.push((t, status));
            }
            for (tick, changes) in &self.expect {
                if *tick != t {
                    continue;
                }
                for (pos, block) in changes {
                    match board.get_mut(&key(*pos)) {
                        Some(cell) => *cell = block.clone(),
                        None => {
                            return Err(format!(
                                "tick {t} の期待 ({}, {}, {}) が領域の外にある",
                                pos.x, pos.y, pos.z
                            ));
                        }
                    }
                }
            }
            for pos in self.region() {
                let expect = &board[&key(pos)];
                let actual = world.get_state(pos).to_string();
                report.compared += 1;
                if *expect != actual {
                    report.mismatches.push(Mismatch {
                        tick: t,
                        pos,
                        expect: expect.clone(),
                        actual,
                    });
                }
            }
        }
        Ok(report)
    }
}

/// JSON を読んで回す。
pub fn run_json(json: &str) -> Result<Report, String> {
    Fixture::parse(json)?.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVER_WIRE_LAMP: &str =
        include_str!("../../../conformance/fixtures/lever-wire-lamp.json");
    const LAMP_OFF_DELAY: &str = include_str!("../../../conformance/fixtures/lamp-off-delay.json");

    fn assert_passes(json: &str) -> Report {
        let report = run_json(json).unwrap();
        assert!(report.passed(), "{report}");
        report
    }

    // レバー → ダスト 3 本 → ランプ。t2 に入れると、同じ tick にダストが 15・14・13 になり、ランプが点く。
    // [実機 fixture lever-wire-lamp。26.3 のオラクルで撮り直しても expect は同じ]
    #[test]
    fn lever_wire_lamp_matches_every_tick() {
        let report = assert_passes(LEVER_WIRE_LAMP);
        // tick 0〜8 の 9 回 × 領域 5 × 2 × 2 = 20 マス。
        assert_eq!(report.compared, 9 * 20);
    }

    // t2 に入れ、t10 に戻す。ダストは t10 に 0 へ戻り、ランプは 4gt 後の t14 に消える。
    // [実機 fixture lamp-off-delay。26.3 のオラクルで撮り直しても expect は同じ]
    #[test]
    fn lamp_off_delay_matches_every_tick() {
        let report = assert_passes(LAMP_OFF_DELAY);
        assert_eq!(report.compared, 19 * 20);
    }

    // 合わない時は、tick と座標と両方の文字列が出る。
    // 期待から t14 の消灯を抜くと、t14 から最後まで、ランプの 1 マスだけが合わない。
    // (読み込んだ fixture の写しを変えるだけで、ファイルの expect には触れない。)
    #[test]
    fn mismatch_reports_tick_and_position() {
        let mut fixture = Fixture::parse(LAMP_OFF_DELAY).unwrap();
        fixture.expect.retain(|(tick, _)| *tick != 14);
        let report = fixture.run().unwrap();
        assert!(!report.passed());
        let ticks: Vec<u32> = report.mismatches.iter().map(|m| m.tick).collect();
        assert_eq!(ticks, [14, 15, 16, 17, 18]);
        for m in &report.mismatches {
            assert_eq!(m.pos, Pos::new(4, 1, 0));
            assert_eq!(m.expect, "redstone_lamp[lit=true]");
            assert_eq!(m.actual, "redstone_lamp[lit=false]");
        }
        let shown = report.to_string();
        assert!(shown.contains(
            "tick 14 (4, 1, 0): 期待 redstone_lamp[lit=true] / 実際 redstone_lamp[lit=false]"
        ));
    }

    // 入力を 1 tick 遅らせると、その tick の全部の変化が合わなくなる (入力の位相を見ている)。
    #[test]
    fn late_input_is_caught() {
        let mut fixture = Fixture::parse(LEVER_WIRE_LAMP).unwrap();
        fixture.inputs[0].0 = 3;
        let report = fixture.run().unwrap();
        let at_tick_2: Vec<Pos> = report
            .mismatches
            .iter()
            .filter(|m| m.tick == 2)
            .map(|m| m.pos)
            .collect();
        assert_eq!(
            at_tick_2,
            (0..=4).map(|x| Pos::new(x, 1, 0)).collect::<Vec<_>>()
        );
        assert!(report.mismatches.iter().all(|m| m.tick == 2));
    }

    // 初めの盤面は tick −1 の状態で、t = 0 では tick を進めない。進めるのは t ≧ 1 の `ticks` 回だけ。
    // 今の 3 素子では、t = 0 の前に 1 回進めても盤面に差が出ない (予約を持ち込めない) ので、進めた回数を直に確かめる。
    #[test]
    fn tick_zero_applies_inputs_without_ticking() {
        assert_eq!(run_json(LEVER_WIRE_LAMP).unwrap().ticked, 8);
        assert_eq!(run_json(LAMP_OFF_DELAY).unwrap().ticked, 18);
        // t0 に入れる。ticks = 0 なら 1 回も進めずに、入力の直後の盤面を比べる。
        let mut fixture = Fixture::parse(LEVER_WIRE_LAMP).unwrap();
        fixture.inputs[0].0 = 0;
        fixture.expect[0].0 = 0;
        fixture.ticks = 0;
        let report = fixture.run().unwrap();
        assert!(report.passed(), "{report}");
        assert_eq!(report.ticked, 0);
        assert_eq!(report.compared, 20);
    }

    // 19 §4.2 の「点検」: 初めの盤面を置き、入力なしで 8 tick 空回しして、何も変わらないことを確かめる。
    // 「そのまま」の読み込みは、初めの盤面が静かであることが前提である。状態コードも立たない。
    #[test]
    fn initial_boards_are_quiet_for_eight_ticks() {
        for json in [LEVER_WIRE_LAMP, LAMP_OFF_DELAY] {
            let mut fixture = Fixture::parse(json).unwrap();
            fixture.inputs.clear();
            fixture.expect.clear();
            fixture.ticks = 8;
            let report = fixture.run().unwrap();
            assert!(report.passed(), "{report}");
            assert_eq!(report.compared, 9 * 20);
        }
    }

    // 読まない欄があれば、エラーで止める。黙って読み飛ばさない。
    #[test]
    fn unread_fields_are_refused() {
        let with = |from: &str, to: &str| {
            assert!(LEVER_WIRE_LAMP.contains(from), "{from}");
            run_json(&LEVER_WIRE_LAMP.replacen(from, to, 1)).unwrap_err()
        };
        assert_eq!(
            with(
                r#""ticks": 8,"#,
                r#""ticks": 8, "trustAuthored": true, "skipUntil": 3,"#
            ),
            "fixture に読まない欄がある: skipUntil, trustAuthored"
        );
        assert_eq!(
            with(r#""block": "stone""#, r#""block": "stone", "items": []"#),
            "blocks の項目 に読まない欄がある: items"
        );
        assert_eq!(
            with(r#""action": "use""#, r#""action": "use", "face": "up""#),
            "inputs の項目 (use) に読まない欄がある: face"
        );
        for extra in ["scheduled", "comparators", "cooldowns", "skipReason"] {
            let err = with(r#""ticks": 8,"#, &format!(r#""ticks": 8, "{extra}": [],"#));
            assert_eq!(err, format!("fixture に読まない欄がある: {extra}"));
        }
    }

    // 表に無いブロックと知らない入力は、エラーで止める。
    #[test]
    fn unknown_blocks_and_inputs_are_refused() {
        let base = r#"{"name":"x","mcVersion":"26.3","ticks":1,
            "region":{"from":[0,0,0],"to":[0,0,0]},"blocks":BLOCKS,"inputs":INPUTS,"expect":[]}"#;
        let with = |blocks: &str, inputs: &str| {
            run_json(&base.replace("BLOCKS", blocks).replace("INPUTS", inputs))
        };
        let err = with(
            r#"[{"pos":[0,0,0],"block":"repeater"},{"pos":[0,1,0],"block":"piston"}]"#,
            "[]",
        )
        .unwrap_err();
        assert_eq!(err, "状態表に無いブロック: repeater, piston");
        let err = with("[]", r#"[{"tick":0,"pos":[0,0,0],"action":"place"}]"#).unwrap_err();
        assert_eq!(err, "対応していない入力: place");
        assert!(with("[]", "[]").unwrap().passed());
        assert!(run_json("{").is_err());
    }

    /// 0 から n-1 までの並べ替えを全部作る (辞書順)。
    fn permutations(n: usize) -> Vec<Vec<usize>> {
        if n == 0 {
            return vec![Vec::new()];
        }
        let mut out = Vec::new();
        for rest in permutations(n - 1) {
            for at in 0..n {
                let mut next = rest.clone();
                next.insert(at, n - 1);
                out.push(next);
            }
        }
        out.sort();
        out
    }

    // 初めの盤面を、置く入力で組む。fixture 2 本とも、床 (y=0) の 10 マスを更新なしで置いてから、
    // 上 (y=1) の 5 個 (レバー・ダスト 3 本・ランプ) を 120 通りの順で置く。
    // - どの順でも、領域の全マスが fixture の初めの盤面と文字列まで同じになる。予約は 0 で、状態コードも立たない。
    //   ダストの要求は既定の状態 (4 方向 none) で、形は置いた時の規則と形状更新が決める。
    // - そこから fixture の入力で回すと、更新なしで読んだ世界 (上の試験が expect と比べている世界) と、
    //   毎 tick 全マスが同じになる。
    // fixture のファイルは変えない。入力の action に置くを足さない (実機のハーネスに無い)。
    #[test]
    fn initial_boards_can_be_built_by_placing_in_any_order() {
        let orders = permutations(5);
        assert_eq!(orders.len(), 120);
        for json in [LEVER_WIRE_LAMP, LAMP_OFF_DELAY] {
            let fixture = Fixture::parse(json).unwrap();
            let (floor, upper): (Vec<_>, Vec<_>) = fixture
                .blocks
                .iter()
                .copied()
                .partition(|(pos, _)| pos.y == 0);
            assert_eq!((floor.len(), upper.len()), (10, 5));
            let read_region = |world: &rse_engine::World| -> Vec<String> {
                fixture
                    .region()
                    .map(|pos| world.get_state(pos).to_string())
                    .collect()
            };
            // 比べる相手: 盤面をそのまま置いた世界。
            let settled = || {
                let mut world = new_world(Config::default());
                for (pos, state) in &fixture.blocks {
                    assert!(world.set_state_raw(*pos, *state));
                }
                world
            };
            let initial = read_region(&settled());
            for order in &orders {
                let mut world = new_world(Config::default());
                for (pos, state) in &floor {
                    assert!(world.set_state_raw(*pos, *state));
                }
                for i in order {
                    let (pos, state) = upper[*i];
                    // 周りで決まる値を捨てた要求にする。レバーは face と facing を運ぶので、そのまま。
                    let requested = if state.block() == rse_engine::rse_core::state::block::LEVER {
                        state
                    } else {
                        state.block().default_state()
                    };
                    let out = world.input(
                        InputSection::A,
                        Input::Place {
                            pos,
                            state: requested,
                        },
                    );
                    assert!(out.accepted, "{} {order:?}", fixture.name);
                    assert!(out.status.is_ok(), "{} {order:?}", fixture.name);
                }
                assert_eq!(read_region(&world), initial, "{} {order:?}", fixture.name);
                assert_eq!(world.scheduled_tick_count(), 0);
                assert_eq!(world.game_time(), 0);

                let mut twin = settled();
                for t in 0..=fixture.ticks {
                    if t > 0 {
                        assert!(world.tick().is_ok() && twin.tick().is_ok());
                    }
                    for marked in fixture.inputs_at(t) {
                        let a = world.input(marked.section, marked.input);
                        let b = twin.input(marked.section, marked.input);
                        assert_eq!(a, b);
                        assert!(a.status.is_ok());
                    }
                    assert_eq!(
                        read_region(&world),
                        read_region(&twin),
                        "{} {order:?} tick {t}",
                        fixture.name
                    );
                    assert_eq!(world.scheduled_tick_count(), twin.scheduled_tick_count());
                }
            }
        }
    }
}
