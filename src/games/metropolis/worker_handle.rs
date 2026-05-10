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
//! - `take_action()`: 直近 onmessage で来た response を取り出す。**id が
//!   一致した時だけ** `in_flight` をクリアして `AiAction` を返す。id 不一致や
//!   parse 失敗は drop して `in_flight` はそのまま (= 本来の応答の到着を待つ)。
//! - `try_dispatch()`: `in_flight = None` の時だけ snapshot を送る。`in_flight`
//!   が `STALE_TIMEOUT_TICKS` を超えて滞留していたら Worker 死亡とみなし強制解除
//!   して新規 dispatch を許可する。
//!
//! ## 失敗時の挙動
//!
//! Worker 生成自体に失敗 (file:// 起動・worker 制限環境等) しても致命では
//! ないので `try_new` が `None` を返す。`MetropolisGame` は `None` の時
//! 同期パス (`logic::tick`) にフォールバックする。
//!
//! ### in_flight ロック防止
//!
//! Worker init 失敗 / `ai_decide` 例外 / parse 失敗で応答ゼロが続いても、
//! `try_dispatch` の `STALE_TIMEOUT_TICKS` 超過で `in_flight` を強制解除する。
//! 強制解除後に古い id 応答が遅れて到着しても、`take_action` の id 一致判定で
//! drop されるため二重適用にならない (= cascade での fresh dispatch 喪失も無い)。

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
/// 10 Hz の tick で 10 秒 = 100 tick。Tier 5 の AI が数百 ms で返ってくる
/// 想定 + Worker WASM の cold start (低速モバイルで 1〜2 秒) を吸収できる
/// 余裕を確保した。これを超えたら Worker 側の永久失敗 (init 失敗・例外で
/// postMessage 不能) とみなし `try_dispatch` で強制解除する。
const STALE_TIMEOUT_TICKS: u64 = 100;

pub struct AiWorkerHandle {
    worker: Worker,
    /// onmessage で受信した response JSON の置き場。`take_action` が `take()` する。
    inbox: Rc<RefCell<Option<String>>>,
    /// 次回発番する request_id。`u32::MAX` を超えたら 1 から再利用。
    next_request_id: u32,
    /// in-flight 状態。`Some((request_id, dispatched_at_tick))` で投げっぱなしの
    /// request を表す。`None` = 新しい request を送れる。
    /// id と dispatch_tick を 1 つの Option にまとめることで、強制解除パスで
    /// 片方だけ stale に残るリスクを潰している。
    in_flight: Option<InFlight>,
    /// `Closure` を Drop すると Worker から listener が外れて自動キャンセル
    /// されるため、handle が生きている間は保持し続ける。
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

#[derive(Clone, Copy)]
struct InFlight {
    request_id: u32,
    dispatched_at_tick: u64,
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
            _on_message: on_message,
        })
    }

    /// 直近の response を取り出して `AiAction` に復元する。
    ///
    /// **id が一致した時だけ** `in_flight` をクリアする。古い id (例: timeout
    /// で強制解除された後に遅れて到着した応答) は drop し、`in_flight` には
    /// 触れない — 本来の現 in-flight 応答が到着するチャンスを残すため。
    /// parse 失敗時も同様に in_flight を維持し、`try_dispatch` の timeout が
    /// 最終救済する。
    pub fn take_action(&mut self) -> Option<AiAction> {
        let raw = self.inbox.borrow_mut().take()?;
        let (id, action) = ai_worker::parse_response_json(&raw).ok()?;
        let current = self.in_flight?;
        if current.request_id != id {
            // stale 応答 — 強制解除→再 dispatch 後に来た古い id。drop だけ。
            return None;
        }
        self.in_flight = None;
        Some(action)
    }

    /// in-flight が無ければ現在の `City` を snapshot して送信する。
    ///
    /// `STALE_TIMEOUT_TICKS` を超えた in_flight は Worker 永久失敗とみなし
    /// 強制解除する。Worker `init()` 失敗で postMessage が永久に来ないケースを
    /// ここで救済する。
    pub fn try_dispatch(&mut self, city: &City) {
        if let Some(current) = self.in_flight {
            let elapsed = city.tick.wrapping_sub(current.dispatched_at_tick);
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
            self.in_flight = Some(InFlight {
                request_id: id,
                dispatched_at_tick: city.tick,
            });
        }
    }
}
