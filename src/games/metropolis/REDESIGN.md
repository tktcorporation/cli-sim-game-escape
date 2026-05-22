# Metropolis 再設計ドキュメント — 「停滞しない街と熱くならない端末」

> 本ドキュメントは Idle Metropolis の **AI 評価関数と探索** を上流から再設計する
> ための指針。既存の `DESIGN.md` (Phase A/B/C のフェーズ計画) を補完し、現行 AI が
> 抱える「街が育たない」「端末が熱くなる」の 2 大症状を根治することが目的。
>
> 実装は段階的に進める。各ステップで `cargo test` と `simulator` ベンチが
> 通ることが必須。

---

## 1. 問題の所在 — シミュレータが示した不安定性

### 1.1 観測データ (Tier 4 / 30min / Income / workers=4 / seed=0xC1A5_5EED)

| t      | pop  | built | cash    | inc/s | 状態 |
| ------ | ---- | ----- | ------- | ----- | --- |
| 0s     | 0    | 0     | $200    | 0     | スタート |
| 60s    | 12   | 12    | $4      | 1     | 早々に cash 枯渇 |
| 300s   | 12   | 32    | $9      | 1     | **停滞期: Road だけ建てる** |
| 600s   | 16   | 49    | $24     | 1     | まだ House が増えない |
| 900s   | 144  | 87    | $23     | 15    | ようやく離陸 |
| 1200s  | 916  | 146   | $2.9K   | 147   | 急加速 |
| 1500s  | 1252 | 167   | $15.7K  | 232   | ピーク |
| **1800s** | 1300 | **174** | $75.7K | 230   | **built が止まる、cash 暴騰** |

**Applied actions 内訳 (5min バケット)**:
- 0–300s: Road×29 + House×13 + Mall×1 + Park×1 → **Road 量産**
- 300–600s: Road×16 + House×10
- 1200–1500s: **Demolish×33 + Replace→MegaMall×11** + Refinery×6
- 1500–1800s: Demolish×7 + Build MegaMall×6 → **振動状態**

**Idle (cash >= $2000) の発生件数**: 11714 actions 中 **2738 件** = AI が「打つ手なし」と
判断している時間が膨大。プレイヤー視点では「街が止まって見える」「AI が考えていない」。

### 1.2 「端末が熱くなる」の正体

`evaluate()` = `compute_income_per_sec_cents` (10K ops) + `road_network_value` (5K ops)
+ `frontier_potential_value` (BFS) + `inactive_building_penalty` (3K ops)
+ `outpost_territory_bonus` (3K ops) + `strategy_thematic_bonus` (3K ops) ≈ **25K ops / evaluate**。

Tier 4 (depth=2, K=4) で 1 tick あたり:
- `rank_actions_full`: 200 候補 × evaluate = **5M ops**
- `search_best_action_full`: 4 × `best_continuation_value_full`(depth=1) = 4 × 5M = **20M ops**
- **合計 25M ops / tick × 10 ticks/sec = 250M ops/sec**

WASM dlmalloc + per-call alloc churn を加味すると現実速度は 30–50M ops/sec。tick が
100ms 超過 → 1 フレームドロップ → AI Worker への dispatch も詰まる → メインスレッド
ブロック → **JIT が CPU を回し続ける = 端末熱**。

### 1.3 「街が育たない」の構造的原因

1. **序盤の Road バイアス**: `road_network_value` の `FRONTIER_PER_CELL = 8` と
   `frontier_potential_value` (距離 2 で 6、3 で 4…) が、 House を 1 軒建てる Δ
   (~+25–50 cents/sec) より Road を 1 本伸ばす Δ (= 隣接 Empty 4 セル × 8 = 32 cents/sec
   + potential ~12 cents/sec) を上回らせる。 → 「**Road を伸ばし続ける**」のが
   評価関数上の最適手になる。
2. **終盤の振動**: saturated map で「すでに建っている Shop を Mall に Replace」
   の Δevaluate が Build House より大きくなり、AI が**既存セルを建て直し続ける**。
   `built_at_tick` が Replace でリセットされるので、aging penalty も効かない。
3. **手詰まり Idle**: 候補が無くなる (cash 不足 or saturated) と AI は Idle を返す。
   この時 cash があっても「次に打つべき手」が無い → 30 min 中 27% が Idle。

---

## 2. リサーチで得た知見

### 2.1 Utility AI (Game AI Pro, Shaggy Dev, The Sims, Zoo Tycoon)

- **基本**: 各 action に 0..1 の utility score を計算し、max を取る。
- **Consideration の組み合わせは「積」**: `utility = c1 * c2 * c3` のように
  multiplicative にすることで「ある条件で 0 になる (= 完全に却下する)」効果を
  作れる。加算 (additive) だと「一部条件が悪くても他で押し切る」現象が起きる。
- **Response curves**: 線形ではなく非線形 (sigmoid, quadratic, threshold) を使うことで
  urgency をモデル化。例: HP < 30% で healing utility が急上昇。
- **Bucketing**: action を「緊急 / 通常 / 余暇」に分け、緊急 bucket が空でない時のみ
  通常 bucket を見る。saturated map での「やることがある時はそれをやる」に対応。
- **Weighting**: bucket やカテゴリーに >1.0 の倍率を掛けて「再帰なしの優先度」を表現。

### 2.2 Feedback Loops (Machinations, Oakleaf Games, SystemsAndUs)

- **Positive loop の暴走 = stall も含む**: 「強くなって何もできない」も loop 暴走の一形態。
  リソースが偏ると新規行動の余地が消える。
- **対策**: diminishing returns, scaling cost, decay, resource sinks, caps。
- 「**保存税** (= 維持コスト)」が古典的な dampening。SimCity の維持費に相当。
  → 本リポジトリは aging factor (収入減衰) で表現しているが、Replace でリセットされる
  ため効いていない。
- **negative loop は dampener として必須**: 「強くなったら難しくなる」が無いとゲームが
  暴走 (= 終わる or 退屈する)。

### 2.3 SimCity の Growth Stage (SC4D Encyclopaedia, StrategyWiki)

- 各 building は **stage 1..8** を持ち、零地から建つ建物は stage 1 のみ。
  地域人口 + 局所 desirability で高 stage が解禁。
- **demand → desirability → growth** の 3 段階分離。demand は全体、desirability は
  cell 単位、growth は時間遅延付き。
- 本リポジトリの `effective_house_tier(target, age)` はこれに近いが、demand 計算が
  「自身の `house_capacity(Cottage)`」固定で、Tier 連動の人口反映を意図的に
  避けている (循環参照防止)。これ自体は妥当な判断。

### 2.4 Beam Search (Wikibooks, ScienceDirect)

- 「**heuristic が悪いと深く探索しても無駄**」が beam search の本質。
- 本リポジトリの Tier 4/5 で depth>=2 が効いていないのはこの現れ。
- 「広い beam + 良い heuristic + depth=1」が「狭い beam + 凡庸な heuristic + depth=3」
  より良い。

---

## 3. 新設計の原則

### P1. **「停滞しない」が最優先、ペースは遅くてよい**

ユーザーの明示要件: 「街が育つペースはゆっくりでいい。停滞しなければ」。
これは設計の全制約に優先する。

**停滞の定義 (v2)**: 「建設・撤去のいずれの **進捗イベント** も発生せず、かつ
Construction/Clearing で in-progress なタイルも存在しない状態」が続くこと。
建物 1 軒が完成するのに 10 分・30 分・場合によっては 1 日かかる設計を許容する
(= 「次の Build のために cash を貯めている間」も Construction tile があれば停滞ではない)。

**進捗イベント** = 以下のいずれか:
- `start_construction` 成功 (Build/Clearing 着工)
- `demolish_at` 成功 (撤去)
- `advance_construction` で完成・整地完了

**閾値**:
- 任意の **連続 60 分 (= 36000 ticks)** で進捗イベントが 1 件も無く、かつ
  Construction/Clearing タイルが 0 件 → **fail** (= 完全停滞)
- 30 分超 60 分未満は警告レベル (test では report のみ)
- 同じセルへの Build/Demolish/Replace が **60 秒で 3 回以上** → 振動 fail
- AI が `Idle` を選ぶ瞬間に **free_workers == 0 または cash < 直近最安候補のコスト** を満たす
  (= 「cash があるのに何もしない」は禁止、`cheap_action_score` で最安 Build を必ず候補化)。
  ベンチでは簡略化して `cash >= $2000 で Idle` の発生率が **全 actions の 5% 未満**で代替。

これらを `simulator::tests` で不変条件として表現する。

### P2. **AI 評価関数は「実 income」と「停滞検知ペナルティ」のみ**

加算 5 成分を **2 成分**に削減:

```rust
pub fn evaluate(city: &City) -> i64 {
    let connected = cached_edge_connected_roads(city);
    compute_income_per_sec_cents_with(city, &connected)
        + stagnation_penalty(city)
}
```

- `stagnation_penalty`: 「街がここから先伸びる余地」を 0 か負の値で減点。
  - すべての House が edge_connected で Apartment 以上の target を持つ → 0
  - inactive な Shop/Workshop が多い → -50/個
  - 完成 House が 0 → -200 (= 「街が無い」シグナル)

その他の bonus は **`cheap_action_score` に移管** (= 行動選択前のヒューリスティック)。
これで evaluate のコストが 25K → 10K ops に削減 (60% 削減)。

### P3. **探索深さは Tier に関係なく depth = 1**

`search_best_action_full` (depth=2/3) を **廃止**。Tier の差別化は:

| Tier | 評価関数 | 候補数 | ノイズ | 特徴 |
| --- | --- | --- | --- | --- |
| Random (1) | なし | enumerate | 100% | 「街が止まらない」最低限のみ |
| Greedy (2) | `evaluate_simple` (House数) | top 6 | 30% | 単純な駒得 |
| Aware (3) | `evaluate` | top 8 | 5% | フル評価 |
| Planner (4) | `evaluate` + **stage bias** | top 20 | 0% | 段階制ヒント |
| Master (5) | `evaluate` + **stage bias** + **continuation hint** | top 30 | 0% | 中期計画 |

- **stage bias**: 街の段階 (`city_tier_for(population)`) に応じて `cheap_action_score` の
  重みを変える。Village なら House 重視、Town なら Shop、City なら Mall/Office、
  Metropolis なら HQ/MegaMall。
- **continuation hint**: depth=1 のままで「次に何を建てたいか」を 1 step lookahead
  ではなく "expected next action" として現在の score に加える (= score の future
  potential を込みで評価)。実装は depth=2 より遥かに軽量。

これで「探索」より「**広く正確に評価する**」方向に Tier を組み直す。

### P4. **enumerate_actions の Replace を厳格に絞る**

Replace 候補は以下のみ:

1. `current` が **inactive** な建物 (Shop/Workshop/Office/Mall 系で `*_is_active` が false)
2. `current` が **同系列の下位**であり、`kind` がその直接上位 (Shop→Mall, Mall→MegaMall,
   Workshop→Factory, Factory→Refinery, Office→HQ)

「異種への Replace」 (Shop → House など) は **常に禁止**。これで saturated map での
Replace 候補が 13 × N_built → 約 1 × N_inactive + 1 × N_upgradable に激減。

### P5. **Stagnation Breaker (停滞ブレーカー) を AI に組み込む**

`drive_ai` の入口で「直近 N tick で何も新規 build / 完成 が無い」状態を検知し、
**stagnation mode** に入る:

- enumerate_actions に通常フィルタを当てずに「現在の cash で買える Build 候補全件」を渡す
- evaluate を使わず、`cheap_action_score` の上位から random 1 つ (Tier 1 風)
- これで「AI が一時的に頭が悪くなって何か建てる」が保証される

stagnation 状態は **連続 60 ticks (= 6 秒) で何も完成しない** で発火、Build 完成と同時に解除。
これは P1 の不変条件を「設計レベルで保証する」最後の砦。

### P6. **Aging factor の Replace リセットを廃止**

`built_at_tick` を Replace 時にリセットしない (= 同セルに連続して建てても築年数は継続)。
これで「Mall→MegaMall に Replace すると aging が 0 にリセット」のループを断ち切り、
終盤の Replace 振動を **dampening** で抑制する (= positive loop に対する negative loop)。

---

## 4. 具体的な変更計画

### 4.1 logic.rs

| 関数 | 変更 |
| --- | --- |
| `evaluate()` | 5 成分→2 成分 (income + stagnation_penalty) |
| `stagnation_penalty()` | **新規**。inactive 建物・無 House を減点 |
| `road_network_value()` | **削除** (cheap_action_score へ統合) |
| `frontier_potential_value()` | **削除** (BFS コスト除去) |
| `outpost_territory_bonus()` | **削除** (cheap_action_score へ統合) |
| `strategy_thematic_bonus()` | **削除** (cheap_action_score へ統合) |
| `search_best_action_full()` | **削除** |
| `best_continuation_value_full()` | **削除** |
| `rank_actions_full()` | 互換維持 (depth=1 のみ呼ばれる) |
| `cheap_action_score()` | **stage bias** を追加。street/house/shop/highrise の段階で重みを変える |
| `enumerate_actions()` | Replace を P4 のルールで厳格に絞る |
| `apply_ai_action()` | Replace 時に `built_at_tick` をリセットしない (P6) |
| `start_construction()` | Replace 経路で既存 `built_at_tick` を継承 |
| `with_action_applied()` | Replace 仮想 mutation でも `built_at_tick` を継承 |

### 4.2 ai.rs

| 関数 | 変更 |
| --- | --- |
| `tier4_planner()` | `search_best_action_full(2, 4)` → `rank_actions_full(20)` の top-1 |
| `tier5_master()` | `search_best_action_full(3, 4)` → `rank_actions_full(30)` の top-1 + continuation hint |
| `decide()` | 入口で `is_stagnant(city)` を見て stagnation_breaker に分岐 |
| `stagnation_breaker()` | **新規**。P5 の動作 |

### 4.3 state.rs

停滞検知用フィールドは当初 Step 1 で追加したが、Step 4–6 で AI が構造的に
停滞しなくなり読み手が無くなったため Step 7 と共に削除した (§5 Step 7 参照)。
state.rs への恒久的なフィールド追加は無し。

### 4.4 simulator.rs

| テスト | 内容 |
| --- | --- |
| `no_stagnation_window_for_tier4_30min` | 進捗イベント (着工/撤去/完成) 間隔が 30min 未満 |
| `no_oscillation_at_same_cell_tier4_30min` | 同セルへの Build/Demolish/Replace が 60 sec 内に 3 回未満 |
| `idle_with_cash_under_5pct_tier4_30min` | `Idle with cash >= $2000` の発生率が全 actions の 5% 未満 |
| `no_stagnation_across_seeds_tier4` | 4 seed × 10min で停滞・振動が起きない頑健性 |

いずれも `#[ignore]` 付き。ベンチが重いため CI では実行せず、リグレッション
調査時に `--ignored` で手動実行する (§5 Step 8 参照)。

### 4.5 既存テスト

- `tier_ordering_holds_at_30min`: 維持 (cash $32K T3 / $48K T4 / $44K T5 → 新ベンチで再評価)
- `tier4_strategies_specialize`: 維持
- `eco_strategy_does_not_stall`: 維持
- `automation_drives_outposts_and_demolitions`: Replace 絞り込みで一部閾値を緩める可能性

---

## 5. 実装ステップ (段階的に cargo test を通しながら)

### Step 1: 計測インフラ (state.rs + simulator.rs)
- `last_progress_tick` / `stagnation_started_tick` を追加
- 新規 simulator テスト 3 件を **#[ignore] + 旧コード基準** で先に書き、現状の数値を記録
- → 「ベースライン記録」コミット

### Step 2: 評価関数の軽量化 (logic.rs)
- `evaluate()` を `income + stagnation_penalty` に変更
- 削除した bonus を `cheap_action_score()` に統合
- 既存テスト全部通す
- → 「evaluate を 5 成分→2 成分に削減」コミット

### Step 3: 探索の廃止 (logic.rs + ai.rs)
- `search_best_action_full` を呼ばない構造に変える
- Tier 4/5 を `rank_actions_full(top_k)` の top-1 に書き換え
- → 「探索深さを depth=1 に統一」コミット

### Step 4: Replace 絞り込み (logic.rs)
- `enumerate_actions` の Replace 生成を P4 ルールに置き換える
- → 「Replace 候補を inactive と直接上位に限定」コミット

### Step 5: Aging リセット廃止 (logic.rs) ✅
- Replace を `apply_replace_preserving_age()` ヘルパーに集約し、`apply_ai_action`
  (worker 経路) と `drive_ai_with_observer` (同期経路) の両方から呼ぶ。`built_at_tick`
  を旧建物から継承し、座標 bounds check を冒頭に置く。

### Step 6: stage bias (logic.rs) ✅ ★ pop 停滞を解消した本命
- `CityStage` (Seed/Village/Town/City/Metropolis) と `city_stage()` を新設。
- `cheap_action_score()` に `stage_weight()` を掛けて段階別の建物優先度を表現。
- Build Road を Metropolis 以前は `-10` 固定 (cash 不足 Road 量産デッドロック防止)。
- `demolish_cleanup_hint()` で死に建物に撤去 hint を与えるが、AI 自身が建てた
  建物 (`built_at_tick > 0`) には 60sec クールダウンを適用し Build→Demolish 振動を防ぐ。
- `neighborhood_match_factor()` で商業/雇用建物に Road 隣接 + 規模相応の House 数を要求。

### Step 7: stagnation breaker — **不要と判断しスキップ** ✅
- 当初は「ランタイムで停滞検知 → 救済モード」を予定していたが、Step 4–6 で AI が
  **構造的に停滞しなくなった**ため不要と結論。`no_stagnation_across_seeds_tier4`
  (4 seed × 10min) で max_gap 12–20s / 振動 0 を実証。
- 停滞検知用フィールド (`last_progress_tick` / `stagnation_started_tick`) は読み手が
  無くなったため削除 (YAGNI)。将来ランタイム検知が必要になれば再導入する。

### Step 8: 不変条件テストの運用方針 ✅
- `no_stagnation_window_for_tier4_30min` / `no_oscillation_at_same_cell_tier4_30min`
  / `idle_with_cash_under_5pct_tier4_30min` / `no_stagnation_across_seeds_tier4` は
  **`#[ignore]` 維持**。30min / multi-seed ベンチは CI を遅延させるため、リグレッション
  調査時に `--ignored` で手動実行する運用とする。
- 軽量な回帰 (`tier_ordering_holds_at_30min` 等、event-driven sim で高速) は通常の
  CI で常時実行され続ける。

各ステップ後に `cargo test` / `cargo clippy -- -W clippy::all` を実行し、
warning が出たら次に進まない。

---

## 6. 期待される結果と検証

### 6.1 シミュレータ実測結果 (Tier 4 / 30min / Income / seed=0xC1A5_5EED)

| 指標 | 旧 | 新目標 | **実測 (達成)** |
| --- | --- | --- | --- |
| 進捗イベント間隔 max | (未測定) | < 30min | **25s** ✅ |
| 同セル 3 回振動 (60s) | あり (1200–1800 sec) | なし | **0 件** ✅ |
| Idle with cash >= $2000 | 2738/11714 (23%) | < 5% | **0% (0/8153)** ✅ |
| 30min pop | 1300 | 100–1300 | **1264** ✅ |
| 30min cash | $75K | $5K–$80K | **$219** (低めだが振動なし) |
| 30min built | 174 (頭打ち) | 継続増加 | **370 (連続増加)** ✅ |
| 30min income/s | 230 | — | **$45** |
| 多 seed 頑健性 (4 seed × 10min) | (未測定) | 停滞・振動なし | **max_gap 12–20s / 振動 0** ✅ |

### 6.2 端末熱対策の指標 (Tier 4 release build)

| 指標 | 旧 | 新目標 |
| --- | --- | --- |
| 1 tick AI コスト | ~25M ops | **< 5M ops** |
| `evaluate()` 呼出回数/tick | ~1000 (depth=2) | < 30 (depth=1 only) |
| BFS 呼出回数/tick | ~200 (frontier_potential 込み) | < 10 (connected_cache + 軽量化) |

これらは `cargo test --release diagnose_t4_30min -- --ignored --nocapture` で
壁時計時間の改善として観察できる (旧: 11min / 新目標: < 3min)。

---

## 7. リスク・トレードオフ

### R1. 深さ探索の廃止で「巧妙な手」が消える可能性
- 「Road を引いて House を建てる」の 2 手読みは現状 Tier 4 から。これを廃止すると
  序盤の判断が悪化する恐れ。
- 緩和策: **stage bias** で Village 段階に強制的に Road + House を優先させる。

### R2. evaluate の単純化で「微妙な手」が見えなくなる
- 例: Park を 1 つ建てるだけで近隣 House が Highrise 化する局面。
- 緩和策: `cheap_action_score` で Park の「促進ボーナス」を表現する。

### R3. Replace 絞り込みで Master AI の「再開発」が出来なくなる
- 既存テスト `drive_ai_demolishes_inactive_shop` は問題なく通る (inactive Shop は Replace 対象)。
- ただし「active Shop を MegaMall に置き換える」終盤戦略は無くなる。
- 緩和策: **同系列の上位 Replace は許可** (P4 ルール 2)。

### R4. Aging リセット廃止で「建て直しが効かない」問題
- 古い House を撤去して新規 House を建てた場合は `built_at_tick` リセット (= Demolish 経由なので OK)。
- 連続 Replace のみ aging が引き継がれる (= 連続 Replace の動機を失わせる)。
- これは設計意図。

---

## 8. 参考文献

- [Utility AI: Introduction (Shaggy Dev)](https://shaggydev.com/2023/04/19/utility-ai/) — 0..1 score, response curves, bucketing, multiplicative considerations
- [Game AI Pro Ch. 9: Utility Theory (Rez Graham)](http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter09_An_Introduction_to_Utility_Theory.pdf) — 業界標準テキスト
- [Game AI Pro Ch. 13: Effective Considerations (Lewis)](http://www.gameaipro.com/GameAIPro3/GameAIPro3_Chapter13_Choosing_Effective_Utility-Based_Considerations.pdf) — weight の罠と response curve 設計
- [Machinations.io: Game Systems and Feedback Loops](https://machinations.io/articles/game-systems-feedback-loops-and-how-they-help-craft-player-experiences) — positive/negative loop の dampening
- [SystemsAndUs: Feedback Loops in Games](https://systemsandus.com/2015/01/04/the-feedback-loops-in-games-what-makes-monopoly-world-of-warcraft-and-mario-kart-so-much-fun/) — loop 暴走と stall の関係
- [SC4D Encyclopaedia: Growth Stage](https://www.wiki.sc4devotion.com/index.php?title=Growth_Stage) — SimCity 4 の building tier 解禁ロジック
- [Beam Search Algorithm (GeeksforGeeks)](https://www.geeksforgeeks.org/machine-learning/introduction-to-beam-search-algorithm/) — heuristic 品質と探索深さの関係
- [Oakleaf Games: Runaway Leader and Rubber Banding](https://oakleafgames.wordpress.com/2014/02/13/game-theory-runaway-leader-rubber-banding-and-feedback/) — 「強者が止まる」現象の分析
