# ADR 0001: Metropolis CPU の振る舞い — 評価関数主導 / ad-hoc ルール禁止 / 診断シミュレーター駆動

- Status: Accepted
- Date: 2026-05-10
- Component: `src/games/metropolis/{ai.rs,logic.rs,simulator.rs}`

## Context

Idle Metropolis は「CPU が勝手に街を作っていくのを眺める」ゲームで、AI の質が体験そのもの。
将棋エンジン風の Tier 別アーキ (`AiTier::Random` … `AiTier::Master`) を採用し、評価関数 +
探索深さ + ノイズ% の3軸で「自然な弱さ」を出す設計を選んだ。

実プレイで以下の症状が継続的に観測されている:

1. **マップ飽和後の完全停滞**: 全セルが埋まると、ワーカーが何も動かなくなる。
   岩盤を撤去すれば拡張余地はあり、未接続建物が大量にあるが整理されない。
2. **コテージのみで埋め尽くされる**: 道路未接続の Cottage が大量に並び、
   Apartment / Highrise への昇格が起きない。
3. **「将棋AI 風」の挙動が不完全**: depth=2/3 の探索を入れているのに、
   「短期に収入が落ちるが長期で伸びる手」を選ばない。

過去に「飽和したらランダムで適当に撤去 → 再建を繰り返す」という症状を抑える対症療法を
入れた結果、enumerate_actions で **House / Park / Plaza / Stadium を Demolish 候補から
完全に除外** する pruning が残った。これにより:

- 道路未接続の Cottage が一度埋まると、AI はそのセルに何もできない (Build 不可・Demolish 不可)
- 全候補が Idle に勝てない → 動きが止まる

## Decision

### 1. CPU の判断は **評価関数 + 探索 + コスト** だけで決まる

「飽和時はランダムで撤去」「Cottage が n 軒以上で再開発」のような **状況分岐ルールを
追加しない**。判断ロジックは以下 3 つに集約する:

- `evaluate(city)`: 局面の良さ (cents/sec + Strategy bonus + 機会コスト)
- `with_action_applied`: 仮想着手 + 評価 + 巻き戻し
- `action_value = Δevaluate − cost`: 統一スコア

この方針は将棋エンジンが「玉の堅さ」「駒得」のような評価項目だけで指し手を決めるのに
対応する。新しい建物を増やしても評価関数が局面を正しく見積もれていれば、AI は自然に
適切な配置を選ぶ。

### 2. enumerate_actions の pruning は **「派生状態」のみで判定**

Demolish 候補を絞る基準は「現在の周辺状況から計算できる派生状態」だけに限る:

- Shop / Workshop 系: `is_active_with(connected)` (= 道路接続 + 需給) で判定
- Road: edge_connected_roads の BFS 結果と隣接 Built の有無
- Outpost: 周囲 Rock の有無
- House: `house_tier_for(neighborhood)` の出力が `Cottage` か (= 周囲が育成条件を
  満たしていない時のみ転用候補)
- Park: 周囲 4 マス以内に House が 1 軒もあるか
- Plaza / Stadium: 終盤の象徴施設として常に保護 (= 「街のランドマーク」)

「House は永続資産」「飽和時は強制シャッフル」のような **設定ベース・状態ベースの
ハードコード分岐は禁止**。判定は周囲のセルから決まる純関数の出力だけを使う。

### 3. 性能は **beam 幅** で抑える、列挙の網羅性は妥協しない

saturated map で候補数が膨らむ問題は、列挙を削ることで解決しない (= 探索の前提が
壊れる)。代わりに:

- `rank_actions` の `top_k` でフィルタ
- `search_best_action` の `beam_width` で深さ方向のフィルタ
- 各候補の `gather_house_neighborhood_with(connected)` のように cache を共有

「列挙されない手は探索深さを増やしても発見されない」原則を死守する。

### 4. 評価関数は **時間軸の長い報酬を「ポテンシャル価値」として加算**

Tier 昇格 dwell time (Apartment 60sec / Highrise 5min / Tower 10min / Arcology 15min)
の利益は、どんな探索深さでも届かない。`evaluate` 単体が局面のスナップショット income
だけを見ている限り、「今 Workshop を建てたら隣の Cottage が将来 Apartment に」のような
判断は 1 手目で正の評価にならない。

evaluate はセル単位の **forecast 値** を加算する設計に拡張する余地を残す:

- 「この Cottage は道路 1 本 + Workshop 追加で Apartment 化候補」→ 期待 income の何割か
- 「この Road は連結すれば N 軒の House を edge_connected にできる」→ 期待 Tier 上昇
- 「この Outpost は M セルの Rock を解禁できる」→ 期待建設価値

これは将棋の positional evaluation (玉の堅さ・駒の働き) と同じ思想。

> 本 ADR では具体的な数式は確定させない。診断シミュレーターでチューニングする
> サイクル (項目 5) で決定する。

### 5. **診断シミュレーターを真実の源泉とする**

「人間が観察して直感で評価関数を弄る」サイクルを廃する。代わりに:

- `simulator.rs` で 30 min / 3 hr / 5 hr 級の長時間シミュレーションを走らせる
- `tick_observed` で AI が打った全アクションをログ記録
- 周期サンプル (cash / pop / built / income / waste) を集める
- `is_stagnant_window`: 停滞窓を機械検出
- `classify_suspicious_action`: 怪しい手を機械検出

評価関数 / 列挙ロジックの変更は **シミュレーター上で挙動が改善することを示してから**
コミットする。新しい建物を追加するとき・新しい戦略を増やすときも同じサイクルを通す。

これは「テストを書いてから実装する」TDD の意思決定版。CPU の挙動は「動くけど質が悪い」
が成立してしまう領域なので、回帰検出の仕組みを最初に置く。

## Consequences

### Positive

- ad-hoc ルールが増えないので、新建物追加時にロジック変更が要らない (`evaluate` の
  数値項目を増やすだけで済む)
- 「列挙が見えていない手」は将来発生しない (= 探索深さを上げれば必ず見つかる)
- 振る舞いの良し悪しがシミュレーターの数値で判定できる (主観評価から脱却)
- ADR で「やってはいけない対症療法」が明文化され、レビューで止められる

### Negative

- enumerate の網羅性を上げる分、saturated map での AI tick が重くなる方向。
  beam 幅と cache 共有で抑える前提だが、性能チューニングが必要
- evaluate の forecast 化は数式設計コストが大きい (バランス調整は dwell time と
  income table に依存)
- 「Cottage が 100 軒並んでいる」状態では再開発判定の cost が無視できない。
  必要なら `gather_house_neighborhood` に row 単位 cache を追加する余地あり

### Neutral

- 探索深さ自体は引き上げない。「3手読めば長期計画が見える」という発想は誤りで、
  3000 tick (= Highrise の dwell) を 3 手で踏破することは不可能。問題は深さでなく
  evaluate の forecast 性

## Implementation milestones

- [x] `enumerate_actions` の pruning を派生状態判定に置き換え (Cottage House / 死に Park
      を Demolish 候補に追加、Plaza / Stadium は保護)
- [x] `tick_observed` を logic.rs に追加 (本番経路は無変更)
- [x] `simulator.rs` に長時間診断ハーネス (`run_diagnostic` / `is_stagnant_window` /
      `classify_suspicious_action`) を追加
- [ ] `evaluate` に forecast 価値項を加算 (Cottage の Apartment 化期待値、Road 接続
      期待効果、Outpost 解禁期待効果)
- [ ] 診断テストを 1 hr / 3 hr horizon に拡張、停滞窓 / 怪しい手の閾値 assertion を入れる
- [ ] saturated map ベンチで Tier 4/5 の AI tick 時間を計測、必要なら beam 幅で調整

## References

- `src/games/metropolis/DESIGN.md` — ゲーム体験の核と建物拡張計画
- `src/games/metropolis/ai.rs` — Tier 別 brain dispatcher
- `src/games/metropolis/logic.rs::evaluate` — 評価関数本体
- `src/games/metropolis/logic.rs::enumerate_actions` — アクション列挙 + pruning
- `src/games/metropolis/simulator.rs::diagnose_t4_30min` — 診断ハーネス
