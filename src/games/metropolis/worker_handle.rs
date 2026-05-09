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
//! 単発の send/receive エラーは握りつぶし、次 tick で再試行する。Worker の
//! `init()` 完了前に postMessage しても Worker 側の `await ready` で順番待ち
//! になるため、起動レースは worker 側で解決される。

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker};

use super::ai::AiAction;
use super::ai_worker;
use super::state::City;

pub struct AiWorkerHandle {
    worker: Worker,
    /// onmessage で受信した response JSON の置き場。`take_action` が `take()` する。
    inbox: Rc<RefCell<Option<String>>>,
    /// 次回発番する request_id。`u32::MAX` を超えたら 1 から再利用。
    next_request_id: u32,
    /// 投げっぱなしの request_id。stale 判定 (response.request_id が一致するか) に使う。
    /// `None` = 無投げ状態 = 新しい request を送れる。
    in_flight: Option<u32>,
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
            _on_message: on_message,
        })
    }

    /// 直近の response を取り出して `AiAction` に復元する。
    ///
    /// stale (古い request の response) は捨てる。in-flight が解消されるのは
    /// 一致したときのみで、stale を取りこぼした in-flight は次の `take_action`
    /// 時に新しいレスポンスが来るまで残り続ける (= 新規 dispatch が走らない)。
    /// この自然待機が「main の state が動いてもワーカーは前回 snapshot で計算
    /// しているだけ」状態を吸収する。
    pub fn take_action(&mut self) -> Option<AiAction> {
        let raw = self.inbox.borrow_mut().take()?;
        let (id, action) = ai_worker::parse_response_json(&raw).ok()?;
        if Some(id) != self.in_flight {
            return None;
        }
        self.in_flight = None;
        Some(action)
    }

    /// in-flight が無ければ現在の `City` を snapshot して送信する。
    pub fn try_dispatch(&mut self, city: &City) {
        if self.in_flight.is_some() {
            return;
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
        }
    }
}
