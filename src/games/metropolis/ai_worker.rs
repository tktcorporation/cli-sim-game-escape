//! AI worker のメインスレッド ↔ Web Worker 通信プロトコル。
//!
//! Web Worker (別 WASM インスタンス) に AI 探索を逃がすことで、
//! `ai::decide` のコスト (Tier 5 で saturated map だと数百 ms 掛かる) が
//! メインスレッドの render 60FPS を巻き込まないようにする。
//!
//! プロトコルは JSON 文字列で 2 種類のメッセージを postMessage する:
//!   - Main → Worker: `Request` (current state snapshot + request_id)
//!   - Worker → Main: `Response` (resolved AiAction + 元 request_id)
//!
//! `request_id` は「同 request の往復」を確認するためのトークン。Main 側で
//! `City` が変わると stale な response を捨てて新しい snapshot を送り直す。
//!
//! スナップショットには既存 `save::extract_save` を再利用する。`GameSave` は
//! 永続化用に既に「AI 判断に必要な全フィールド」を網羅しており、events 等
//! AI に不要なフィールドが少し含まれるが、コードベースを 1 ヶ所で管理する
//! 利点が勝つ。
//!
//! `target_arch = "wasm32"` でも `test` でも有効化することで、ネイティブ
//! テストから protocol roundtrip を検証できる。

#![cfg(any(target_arch = "wasm32", test))]

use serde::{Deserialize, Serialize};

use super::ai::AiAction;
use super::save::{apply_save, extract_save, GameSave};
use super::state::{Building, City};

/// Main → Worker のリクエスト 1 通分。
///
/// 公開 API は `build_request_json` / `handle_request_json` /
/// `parse_response_json` の 3 つだけで、構造体は内部実装。
#[derive(Serialize, Deserialize)]
struct Request {
    snapshot: GameSave,
    request_id: u32,
}

/// Worker → Main のレスポンス 1 通分。
#[derive(Serialize, Deserialize)]
struct Response {
    request_id: u32,
    action: ActionWire,
}

/// AiAction の wire 表現。`ai::AiAction` は serde を持たないので、
/// `Building` を `u8` に正規化した固定形式に変換する。enum variant 数や
/// フィールド数が変わったら全箇所が compile error で落ちるため、
/// プロトコル変更検知の中核も兼ねる。
#[derive(Serialize, Deserialize, Clone, Copy)]
enum ActionWire {
    Build { x: u8, y: u8, kind: u8 },
    Demolish { x: u8, y: u8 },
    Replace { x: u8, y: u8, kind: u8 },
    Idle,
}

impl ActionWire {
    fn from_action(a: &AiAction) -> Self {
        match a {
            AiAction::Build { x, y, kind } => ActionWire::Build {
                x: *x as u8,
                y: *y as u8,
                kind: building_to_u8(*kind),
            },
            AiAction::Demolish { x, y } => ActionWire::Demolish {
                x: *x as u8,
                y: *y as u8,
            },
            AiAction::Replace { x, y, kind } => ActionWire::Replace {
                x: *x as u8,
                y: *y as u8,
                kind: building_to_u8(*kind),
            },
            AiAction::Idle => ActionWire::Idle,
        }
    }

    fn to_action(self) -> Option<AiAction> {
        match self {
            ActionWire::Build { x, y, kind } => building_from_u8(kind).map(|kind| AiAction::Build {
                x: x as usize,
                y: y as usize,
                kind,
            }),
            ActionWire::Demolish { x, y } => Some(AiAction::Demolish {
                x: x as usize,
                y: y as usize,
            }),
            ActionWire::Replace { x, y, kind } => building_from_u8(kind).map(|kind| AiAction::Replace {
                x: x as usize,
                y: y as usize,
                kind,
            }),
            ActionWire::Idle => Some(AiAction::Idle),
        }
    }
}

/// `Building` ↔ `u8` 変換は `save.rs` 側にも独立実装があるが、protocol 互換性は
/// `save.rs` のセーブデータ互換性とは別レイヤー (worker 通信は同バージョン
/// 内で完結) のため、ここで再定義する。意図的に save と独立にすることで、
/// 将来 save の数値割当を変えても worker 通信が壊れない。
fn building_to_u8(b: Building) -> u8 {
    match b {
        Building::Road => 1,
        Building::House => 2,
        Building::Park => 3,
        Building::Workshop => 4,
        Building::Shop => 5,
        Building::Office => 6,
        Building::Factory => 7,
        Building::Mall => 8,
        Building::Outpost => 9,
        Building::Plaza => 10,
        Building::Refinery => 11,
        Building::MegaMall => 12,
        Building::Headquarters => 13,
        Building::Stadium => 14,
    }
}

fn building_from_u8(v: u8) -> Option<Building> {
    Some(match v {
        1 => Building::Road,
        2 => Building::House,
        3 => Building::Park,
        4 => Building::Workshop,
        5 => Building::Shop,
        6 => Building::Office,
        7 => Building::Factory,
        8 => Building::Mall,
        9 => Building::Outpost,
        10 => Building::Plaza,
        11 => Building::Refinery,
        12 => Building::MegaMall,
        13 => Building::Headquarters,
        14 => Building::Stadium,
        _ => return None,
    })
}

/// Main thread が呼ぶ: 現在の `City` を request 1 通分の JSON にする。
///
/// `events` (UI 表示用のログ) は AI 判断に使われないので空にしてから serialize
/// する。saturated map では数十件溜まる文字列なので、毎 tick の postMessage
/// コストにそれなりに効く。
pub fn build_request_json(city: &City, request_id: u32) -> Result<String, serde_json::Error> {
    let save = extract_save(city);
    let mut snapshot = save.game;
    snapshot.events.clear();
    let req = Request {
        snapshot,
        request_id,
    };
    serde_json::to_string(&req)
}

/// Worker entry が呼ぶ: request JSON を受けて AiAction を計算 → response JSON を返す。
///
/// 内部で `City::new()` から freshly な状態を組み立てて `apply_save` で
/// snapshot を流し込み、`ai::decide` を実行する。Worker 内の `City` は
/// 1 通ごとに使い捨て (RC キャッシュも fresh) なので main の City とは独立。
pub fn handle_request_json(req_json: &str) -> Result<String, serde_json::Error> {
    let req: Request = serde_json::from_str(req_json)?;
    let mut city = City::new();
    apply_save(&mut city, &req.snapshot);
    let action = super::ai::decide(&mut city);
    let resp = Response {
        request_id: req.request_id,
        action: ActionWire::from_action(&action),
    };
    serde_json::to_string(&resp)
}

/// Main thread が呼ぶ: response JSON を解いて `(request_id, AiAction)` を返す。
pub fn parse_response_json(resp_json: &str) -> Result<(u32, AiAction), ParseError> {
    let resp: Response = serde_json::from_str(resp_json).map_err(ParseError::Json)?;
    let action = resp.action.to_action().ok_or(ParseError::UnknownBuilding)?;
    Ok((resp.request_id, action))
}

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    UnknownBuilding,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "json parse error: {}", e),
            ParseError::UnknownBuilding => write!(f, "unknown building variant in wire format"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_city() -> City {
        let mut c = City::new();
        c.cash = 5_000;
        c.tick = 100;
        c
    }

    #[test]
    fn roundtrip_idle_action() {
        let city = make_city();
        let req = build_request_json(&city, 7).unwrap();
        let resp = handle_request_json(&req).unwrap();
        let (id, action) = parse_response_json(&resp).unwrap();
        assert_eq!(id, 7);
        // 初期状態 + cash 5000 では Tier 1 がランダムに何か建てる or Idle を返す。
        // どの variant が返るかは ai::decide 依存なので、parse できることだけ確認。
        let _ = action;
    }

    #[test]
    fn building_round_trips_through_wire() {
        for &b in &[
            Building::Road,
            Building::House,
            Building::Park,
            Building::Workshop,
            Building::Shop,
            Building::Office,
            Building::Factory,
            Building::Mall,
            Building::Outpost,
            Building::Plaza,
            Building::Refinery,
            Building::MegaMall,
            Building::Headquarters,
            Building::Stadium,
        ] {
            let v = building_to_u8(b);
            let restored = building_from_u8(v).unwrap();
            assert_eq!(restored, b);
        }
    }

    #[test]
    fn unknown_building_returns_error() {
        let bogus = Response {
            request_id: 1,
            action: ActionWire::Build {
                x: 0,
                y: 0,
                kind: 99,
            },
        };
        let json = serde_json::to_string(&bogus).unwrap();
        match parse_response_json(&json) {
            Err(ParseError::UnknownBuilding) => {}
            other => panic!("expected UnknownBuilding, got {:?}", other.map(|_| "ok")),
        }
    }
}
