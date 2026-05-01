# Dungeon Dive — コアコンセプト: 「もう一部屋だけ…の誘惑と恐怖」

## コア体験

プレイヤーが「もう少し奥に行けばもっといいものが手に入る…」と思いながらも
「でもここで死んだら全部失う…」と葛藤する、この緊張感がゲームの核心。

## プレイヤーの感情

「あと1部屋でボーナス+60G…でもHP残り30%…薬草も残り1個…行くか？帰るか？」

## 主軸

**リスク・リワードの判断** — 奥に進むほど報酬は増えるが、死のリスクも上がる。
生きて帰れば帰還ボーナスが得られるが、欲張って死ねば獲得した金を失う。

## デザイン原則

- **帰還ボーナス**: 生きて町に帰ると、探索した部屋数×階層に応じたボーナス金が得られる
- **リスクの可視化**: HP残量、残りアイテム、帰還ボーナス額を常に表示
- **戦闘の読み合い**: 敵の特殊行動予告で「今シールドを使うべきか」の判断が生まれる
- **属性弱点**: スキルの使い分けで「どのスキルを使うか」の戦略が生まれる
- **演出で感情を動かす**: クリティカルヒット、チャージ攻撃、弱点表示でフィードバックを強化

## 改善項目

### Phase 1: コアループ強化
1. **帰還ボーナス** — 部屋クリア数×階層でゴールドボーナス。死亡時は獲得金全額ロス
2. **クリティカルヒット** — 10%で1.5倍ダメージ。「会心の一撃！」表示
3. **敵のチャージ攻撃** — 一部の敵が「力を溜めている！」→次ターン2倍ダメージ

### Phase 2: 戦闘深化
4. **新スキル4種** — アイスブレード(Lv3)、サンダー(Lv5)、ドレイン(Lv6)、バーサク(Lv8)
5. **属性弱点** — Fire/Ice/Thunder。弱点スキルで1.5倍ダメージ

### Phase 3: 演出強化
6. **バトルログ改善** — ダメージ量に応じた表現バリエーション
7. **雰囲気テキスト** — HP低下時の緊迫表現、階層テーマ性

---

## アーキテクチャ: PlayerAction + BalanceConfig (DI)

シミュレーターと本体が乖離しないように、状態変更は **必ず**
`commands::apply_action(state, PlayerAction::*)` を経由させる。

```
入力源
 ├─ mod.rs::handle_input (キー / クリック)
 └─ simulator.rs::Policy (自動プレイ AI)
        │
        ▼
PlayerAction enum (commands.rs)   ← プレイヤーが取れる行動の単一語彙
        │
        ▼
apply_action(state, action)       ← 唯一のディスパッチャ
        │
        ▼
logic.rs の各純粋関数              ← 単位処理 (内部実装)
```

abyss / cookie の simulator と同じ DI パターン。差分は「rpg は scene-driven
(turn-based) で各 step が 1 アクション」なのに対し abyss は tick-driven
(idle) で 1 tick が複数アクション、という点だけ。

### ルール

1. **入力ハンドラ (`mod.rs::handle_input`) は logic.rs を直接呼ばない。**
   キー / クリックを `PlayerAction` に翻訳して `apply_action` に渡す。
2. **シミュレーター (`simulator.rs`) も同様。** `logic::available_skills`
   のような読み取り専用クエリのみ直接呼んでよい。
3. **新しいプレイヤー操作を増やすときは:**
   - `PlayerAction` に variant を追加する
   - `apply_action` のディスパッチに分岐を足す
   - 必要なら logic.rs に新規関数を実装する
   - これで mod.rs / simulator.rs の両方から自動的に使えるようになる
4. **logic.rs の関数は将来 `pub(super)` 化する候補。** 現状 pub だが
   rpg モジュール外からは呼ばれていない。新規依存を増やさないこと。

### 難易度調整 (`BalanceConfig`)

`balance.rs` の `BalanceConfig` が敵HP/ATK/DEF・報酬・罠ダメージ等の
スカラ倍率を保持し、`RpgState.difficulty` 経由で logic.rs の計算式に
注入される。デフォルト (`BalanceConfig::standard()`) は全倍率 1.0 で
既存挙動と一致する。

プリセット: `standard / easy / hard / brutal`。

### シミュレーター実行 (難易度 tuning)

abyss / cookie と同じ流儀で `#[cfg(test)]` gated。手動 tuning 用の runner
は `#[ignore]` を付けて CI からは外してあるので、`--ignored` で実行する:

```bash
# 既定バランスで Balanced / Cautious / Reckless / NoAction を比較
cargo test --release simulate_dungeon_default -- --ignored --nocapture

# easy / standard / hard / brutal を Balanced policy で横並び
cargo test --release simulate_dungeon_balance_sweep -- --ignored --nocapture

# 複数 seed の平均
cargo test --release simulate_dungeon_seed_average -- --ignored --nocapture
```

`Policy` trait に新しい AI を追加するときは `BalancedPolicy` を真似て
`fn next_action(&mut self, &RpgState) -> Option<PlayerAction>` を実装する。
