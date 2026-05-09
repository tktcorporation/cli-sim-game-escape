//! Main thread 側で AI Web Worker を所有する handle。
//!
//! Worker は別 WASM インスタンス (`bin/metropolis_worker.rs`) で動作する。
//! ここでは postMessage の送信、onmessage で受信した response の inbox 化、
//! `request_id` を使った stale 弾きを担当する。
//!
//! ## 駆動モデル
//!
//! 1 リクエスト 1 アクションの厳密な往復。Main 側の `MetropolisGame::tick`
//! が毎 tick で `take_action()` → `try_dispatch()` を 1 セット呼ぶ:
//!
//! - `take_action()`: 直近 onmessage で来た response があれば取り出して `AiAction` に
//!   復元し、`in_flight = None` に戻す。
//! - `try_dispatch()`: 投げっぱなし (in_flight = Some) なら何もしない、空いていれば
//!   現状の `City` snapshot を JSON 化して `worker.postMessage` で送る。
//!
//! ## 失敗時の挙動
//!
//! Worker 生成自体に失敗 (file:// 起動・worker 制限環境等) しても致命では
//! ないので `try_new` が `None` を返す。`MetropolisGame` は `None` の時
//! 同期パス (`logic::tick`) にフォールバックする。
//!
//! ### in_flight ロックの二重防御
//!
//! Worker が応答を返さない / parse 失敗で消費される / WASM init 失敗で永久に
//! 黙る、いずれの場合も `in_flight` が `Some(...)` のまま固まると新規 dispatch
//! が永久に走らず AI が完全停止する。これを次の二段で防ぐ:
//!
//! 1. `take_action()` は inbox を **取り出した時点で** `in_flight` をクリアする。
//!    parse 失敗 / id 不一致でも `in_flight` は解放され、次 tick で fresh request
//!    を投げ直せる。stale な action は drop するだけで実害無し。
//! 2. `try_dispatch()` は応答ゼロのまま `STALE_TIMEOUT_TICKS` 経過した
//!    `in_flight` を強制解除する。Worker init が失敗した場合の最終救済策。

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};

use super::ai::AiAction;
use super::ai_worker;
use super::state::City;

/// in_flight が解消されないまま放置されたとみなすしきい値 (tick 単位)。
/// 10 Hz の tick で 5 秒 = 50 tick。Tier 5 の AI でも数百 ms で返ってくる
/// 想定なので、これを超えるのは Worker 側の永久的な失敗 (init 失敗・例外で
/// postMessage 不能) を意味する。`try_dispatch` がここで強制解除する。
const STALE_TIMEOUT_TICKS: u64 = 50;

pub struct AiWorkerHandle {
    worker: Worker,
    /// onmessage で受信した response JSON の置き場。`take_action` が `take()` する。
    inbox: Rc<RefCell<Option<String>>>,
    /// 次回発番する request_id。`u32::MAX` を超えたら 1 から再利用。
    next_request_id: u32,
    /// 投げっぱなしの request_id。`take_action` で受信を観測 or `try_dispatch` の
    /// timeout で `None` に戻る。`None` = 新しい request を送れる。
    in_flight: Option<u32>,
    /// 直近 `try_dispatch` 成功時の `city.tick`。`STALE_TIMEOUT_TICKS` 超過の
    /// 強制解除判定に使う。
    dispatch_tick: u64,
    /// `Closure` を Drop すると Worker から listener が外れて自動キャンセル
    /// されるため、handle が生きている間は保持し続ける。
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

impl AiWorkerHandle {
    /// Worker をスポーンする。失敗時は `None`。
    ///
    /// `script_url` は dist 配下から見たパス (例: `"./metropolis_worker_entry.js"`)。
    /// クラシック Worker (= `importScripts` を使う形式) として生成する。
    /// Trunk の `data-type="worker"` は wasm-bindgen を `--target no-modules`
    /// で走らせるため、出力もクラシック互換の IIFE になる。
    pub fn try_new(script_url: &str) -> Option<Self> {
        let worker = Worker::new(script_url).ok()?;

        let inbox: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let inbox_for_handler = inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            // event.data() は JsValue。string の時だけ inbox に置く。空文字や非
            // string は worker 側のエラー or 起動メッセージなので無視。
            if let Some(s) = e.data().as_string() {
                if !s.is_empty() {
                    *inbox_for_handler.borrow_mut() = Some(s);
                }
            }
        });
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        Some(Self {
            worker,
            inbox,
            next_request_id: 1,
            in_flight: None,
            dispatch_tick: 0,
            _on_message: on_message,
        })
    }

    /// 直近の response を取り出して `AiAction` に復元する。
    ///
    /// inbox に何かが入っていた時点で `in_flight` をクリアする。parse 失敗 /
    /// id 不一致 / 空 string でも次の dispatch を許可することで、Worker からの
    /// 1 回の "壊れた応答" で AI が永久停止する事故を防ぐ (in_flight ロック防御 1)。
    /// id 不一致の action は捨てて、次 tick で新しい snapshot を投げ直す。
    pub fn take_action(&mut self) -> Option<AiAction> {
        let raw = self.inbox.borrow_mut().take()?;
        // 「観測した時点で in_flight を解放」が中核。
        let was_in_flight = self.in_flight.take();
        let (id, action) = ai_worker::parse_response_json(&raw).ok()?;
        // 1 in-flight 制約下では基本一致するが、保険として stale 判定を残す。
        if was_in_flight != Some(id) {
            return None;
        }
        Some(action)
    }

    /// in-flight が無ければ現在の `City` を snapshot して送信する。
    ///
    /// `STALE_TIMEOUT_TICKS` を超えた in_flight は Worker 永久失敗とみなし
    /// 強制解除する (in_flight ロック防御 2)。Worker `init()` 失敗で
    /// postMessage が永久に来ないケースをここで救済する。
    pub fn try_dispatch(&mut self, city: &City) {
        if self.in_flight.is_some() {
            let elapsed = city.tick.wrapping_sub(self.dispatch_tick);
            if elapsed > STALE_TIMEOUT_TICKS {
                // Worker が遅れて応答してきても take_action 側の id 不一致で
                // 捨てるため、強制解除しても二重適用にはならない。
                self.in_flight = None;
            } else {
                return;
            }
        }
        let id = self.next_request_id;
        // 0 は「未割当」の慣例として避け、wrap 時も 1 から始める。
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);

        let json = match ai_worker::build_request_json(city, id) {
            Ok(s) => s,
            Err(_) => return,
        };
        if self
            .worker
            .post_message(&JsValue::from_str(&json))
            .is_ok()
        {
            self.in_flight = Some(id);
            self.dispatch_tick = city.tick;
        }
    }
}
