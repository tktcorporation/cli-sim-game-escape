//! 深淵潜行 (Abyss Idle) — game state.
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs に置く。

/// 強化の種類。各強化は累積購入数 (level) を持ち、レベルが上がるほどコストも上昇する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    /// 剣術: ATK +2 / level
    Sword,
    /// 体力: Max HP +10 / level
    Vitality,
    /// 鎧: DEF +1 / level
    Armor,
    /// 必殺: CRIT +1% / level (cap 60%)
    Crit,
    /// 俊敏: ATK speed +5% / level
    Speed,
    /// 回復: HP regen +0.2/sec / level
    Regen,
    /// 金運: gold gain +5% / level
    Gold,
}

impl UpgradeKind {
    pub fn all() -> &'static [UpgradeKind] {
        &[
            UpgradeKind::Sword,
            UpgradeKind::Vitality,
            UpgradeKind::Armor,
            UpgradeKind::Crit,
            UpgradeKind::Speed,
            UpgradeKind::Regen,
            UpgradeKind::Gold,
        ]
    }

    pub fn index(self) -> usize {
        match self {
            UpgradeKind::Sword => 0,
            UpgradeKind::Vitality => 1,
            UpgradeKind::Armor => 2,
            UpgradeKind::Crit => 3,
            UpgradeKind::Speed => 4,
            UpgradeKind::Regen => 5,
            UpgradeKind::Gold => 6,
        }
    }

    pub fn from_index(idx: usize) -> Option<UpgradeKind> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            UpgradeKind::Sword => "剣術",
            UpgradeKind::Vitality => "体力",
            UpgradeKind::Armor => "鎧",
            UpgradeKind::Crit => "必殺",
            UpgradeKind::Speed => "俊敏",
            UpgradeKind::Regen => "回復",
            UpgradeKind::Gold => "金運",
        }
    }

    /// 1レベル分の効果説明 (UI用)。
    pub fn effect(self) -> &'static str {
        match self {
            UpgradeKind::Sword => "ATK+2",
            UpgradeKind::Vitality => "HP+10",
            UpgradeKind::Armor => "DEF+1",
            UpgradeKind::Crit => "CRIT+1%",
            UpgradeKind::Speed => "速度+5%",
            UpgradeKind::Regen => "回復+0.2/秒",
            UpgradeKind::Gold => "金+5%",
        }
    }

    pub fn base_cost(self) -> u64 {
        match self {
            UpgradeKind::Sword => 10,
            UpgradeKind::Vitality => 12,
            UpgradeKind::Armor => 18,
            UpgradeKind::Crit => 50,
            UpgradeKind::Speed => 80,
            UpgradeKind::Regen => 30,
            UpgradeKind::Gold => 60,
        }
    }

    /// レベル毎のコスト成長率。
    pub fn growth(self) -> f64 {
        match self {
            UpgradeKind::Sword => 1.15,
            UpgradeKind::Vitality => 1.14,
            UpgradeKind::Armor => 1.18,
            UpgradeKind::Crit => 1.25,
            UpgradeKind::Speed => 1.30,
            UpgradeKind::Regen => 1.20,
            UpgradeKind::Gold => 1.22,
        }
    }
}

/// 魂の永続強化。死亡しても残り、全体倍率を提供する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoulPerk {
    /// 魂の力: 全攻撃力に +5% / level
    Might,
    /// 鋼の意志: 全HPに +5% / level
    Endurance,
    /// 富の加護: gold +10% / level
    Fortune,
    /// 不死の刻印: 死亡時に魂 +20% / level
    Reaper,
}

impl SoulPerk {
    pub fn all() -> &'static [SoulPerk] {
        &[
            SoulPerk::Might,
            SoulPerk::Endurance,
            SoulPerk::Fortune,
            SoulPerk::Reaper,
        ]
    }

    pub fn index(self) -> usize {
        match self {
            SoulPerk::Might => 0,
            SoulPerk::Endurance => 1,
            SoulPerk::Fortune => 2,
            SoulPerk::Reaper => 3,
        }
    }

    pub fn from_index(idx: usize) -> Option<SoulPerk> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            SoulPerk::Might => "魂の力",
            SoulPerk::Endurance => "鋼の意志",
            SoulPerk::Fortune => "富の加護",
            SoulPerk::Reaper => "不死の刻印",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            SoulPerk::Might => "ATK +5%",
            SoulPerk::Endurance => "HP +5%",
            SoulPerk::Fortune => "金 +10%",
            SoulPerk::Reaper => "死亡時の魂 +20%",
        }
    }

    pub fn base_cost(self) -> u64 {
        match self {
            SoulPerk::Might => 3,
            SoulPerk::Endurance => 3,
            SoulPerk::Fortune => 5,
            SoulPerk::Reaper => 8,
        }
    }

    pub fn growth(self) -> f64 {
        match self {
            SoulPerk::Might => 1.45,
            SoulPerk::Endurance => 1.45,
            SoulPerk::Fortune => 1.50,
            SoulPerk::Reaper => 1.60,
        }
    }
}

/// 現在対峙している敵。スポーンの度に作り直される。
#[derive(Clone, Debug)]
pub struct Enemy {
    pub name: String,
    pub max_hp: u64,
    pub hp: u64,
    pub atk: u64,
    pub def: u64,
    pub gold: u64,
    pub is_boss: bool,
    /// 攻撃クールダウン (tick単位)。0 になると攻撃。
    pub atk_cooldown: u32,
    pub atk_period: u32,
}

/// メイン画面のタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Upgrades,
    Souls,
    Stats,
}

/// ゲームのルート状態。
pub struct AbyssState {
    // ── プレイヤー (ラン中もリセットされない、永続強化分のレベル) ──
    pub upgrades: [u32; 7],
    pub soul_perks: [u32; 4],
    pub souls: u64,

    // ── ラン (1回の冒険) 単位の状態 ──
    pub gold: u64,
    pub floor: u32,
    pub max_floor: u32,
    pub kills_on_floor: u32,
    /// このラン中に撃破した敵の総数。
    pub run_kills: u64,
    /// このランで稼いだ gold の総量 (統計用)。
    pub run_gold_earned: u64,

    /// hero の現在 HP。max_hp は upgrades と soul_perks から導出する。
    pub hero_hp: u64,
    /// hero の攻撃進捗 (tick 単位)。`hero_atk_period` 経過するごとに攻撃。
    pub hero_atk_cooldown: u32,
    /// hero の HP regen の小数累積 (1.0 ごとに 1 HP 回復)。
    pub hero_regen_acc_x100: u32,

    pub current_enemy: Enemy,

    // ── プレイヤー設定 ──
    pub auto_descend: bool,
    pub tab: Tab,

    // ── 永続統計 ──
    pub deepest_floor_ever: u32,
    pub total_kills: u64,
    pub deaths: u64,

    // ── 演出 / ログ ──
    pub log: Vec<String>,
    /// hero が攻撃を受けた直後 (赤フラッシュ用 tick)。
    pub hero_hurt_flash: u32,
    /// 敵が攻撃を受けた直後 (黄フラッシュ用 tick)。
    pub enemy_hurt_flash: u32,
    /// 直近の hero ダメージ表示 (タプル: amount, life_ticks)。
    pub last_enemy_damage: Option<(u64, u32, bool)>, // (amount, ticks_left, is_crit)
    pub last_hero_damage: Option<(u64, u32)>,
    /// 階層遷移の演出 tick 残り (0 なら通常表示)。
    pub descent_flash: u32,

    /// ラン中の総tick (デバッグ・統計用)。
    pub total_ticks: u64,

    /// シンプルRNG。
    pub rng_state: u32,
}

impl AbyssState {
    pub fn new() -> Self {
        let mut s = Self {
            upgrades: [0; 7],
            soul_perks: [0; 4],
            souls: 0,
            gold: 0,
            floor: 1,
            max_floor: 1,
            kills_on_floor: 0,
            run_kills: 0,
            run_gold_earned: 0,
            hero_hp: 0,
            hero_atk_cooldown: 0,
            hero_regen_acc_x100: 0,
            current_enemy: placeholder_enemy(),
            auto_descend: true,
            tab: Tab::Upgrades,
            deepest_floor_ever: 1,
            total_kills: 0,
            deaths: 0,
            log: Vec::new(),
            hero_hurt_flash: 0,
            enemy_hurt_flash: 0,
            last_enemy_damage: None,
            last_hero_damage: None,
            descent_flash: 0,
            total_ticks: 0,
            rng_state: 0xC0FFEE,
        };
        s.hero_hp = s.hero_max_hp();
        s.hero_atk_cooldown = s.hero_atk_period();
        s
    }

    /// 現在の最大 HP (upgrades + soul perks の合算)。
    pub fn hero_max_hp(&self) -> u64 {
        let base = 50 + self.upgrades[UpgradeKind::Vitality.index()] as u64 * 10;
        let endurance_lv = self.soul_perks[SoulPerk::Endurance.index()];
        let mult = 1.0 + endurance_lv as f64 * 0.05;
        ((base as f64) * mult).round() as u64
    }

    pub fn hero_atk(&self) -> u64 {
        let base = 5 + self.upgrades[UpgradeKind::Sword.index()] as u64 * 2;
        let might_lv = self.soul_perks[SoulPerk::Might.index()];
        let mult = 1.0 + might_lv as f64 * 0.05;
        ((base as f64) * mult).round() as u64
    }

    pub fn hero_def(&self) -> u64 {
        2 + self.upgrades[UpgradeKind::Armor.index()] as u64
    }

    /// クリティカル率 (0.0..=0.6)。
    pub fn hero_crit_rate(&self) -> f64 {
        let lv = self.upgrades[UpgradeKind::Crit.index()] as f64;
        (lv * 0.01).min(0.60)
    }

    /// 1 攻撃にかかる tick 数。基本は 12 tick (=1.2秒)、SPD upgrade で短縮。
    pub fn hero_atk_period(&self) -> u32 {
        let lv = self.upgrades[UpgradeKind::Speed.index()];
        let mult = 1.0 + lv as f64 * 0.05;
        let period = (12.0 / mult).round() as u32;
        period.max(3) // 0.3秒/攻撃 (=33攻撃/秒) を下限
    }

    /// HP regen (HP/秒)。
    pub fn hero_regen_per_sec(&self) -> f64 {
        self.upgrades[UpgradeKind::Regen.index()] as f64 * 0.2
    }

    /// gold 取得倍率 (1.0 + upgrades + soul perks)。
    pub fn gold_multiplier(&self) -> f64 {
        let upgrade_lv = self.upgrades[UpgradeKind::Gold.index()];
        let fortune_lv = self.soul_perks[SoulPerk::Fortune.index()];
        1.0 + upgrade_lv as f64 * 0.05 + fortune_lv as f64 * 0.10
    }

    /// 撃破時の魂取得倍率 (Reaper perk 由来)。
    pub fn soul_multiplier(&self) -> f64 {
        let lv = self.soul_perks[SoulPerk::Reaper.index()];
        1.0 + lv as f64 * 0.20
    }

    /// 強化 1 段階のコスト。
    pub fn upgrade_cost(&self, kind: UpgradeKind) -> u64 {
        let lv = self.upgrades[kind.index()] as f64;
        let cost = (kind.base_cost() as f64) * kind.growth().powf(lv);
        cost.round() as u64
    }

    pub fn soul_perk_cost(&self, perk: SoulPerk) -> u64 {
        let lv = self.soul_perks[perk.index()] as f64;
        let cost = (perk.base_cost() as f64) * perk.growth().powf(lv);
        cost.round() as u64
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    /// ボス出現までの残り敵数。0 なら現在のフロアにはボスが出現中。
    pub fn enemies_until_boss(&self) -> u32 {
        let needed = enemies_per_floor();
        needed.saturating_sub(self.kills_on_floor)
    }
}

/// 1階層あたり、ボス出現前に倒す必要がある雑魚の数。
pub const fn enemies_per_floor() -> u32 {
    8
}

/// hp/max_hp が 0 だと "次の tick で本物の敵をスポーン" の合図になる。
fn placeholder_enemy() -> Enemy {
    Enemy {
        name: String::new(),
        max_hp: 0,
        hp: 0,
        atk: 1,
        def: 0,
        gold: 0,
        is_boss: false,
        atk_cooldown: 20,
        atk_period: 20,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_sane() {
        let s = AbyssState::new();
        assert_eq!(s.floor, 1);
        assert!(s.hero_hp > 0);
        assert_eq!(s.hero_hp, s.hero_max_hp());
        assert_eq!(s.gold, 0);
        assert_eq!(s.souls, 0);
    }

    #[test]
    fn upgrades_change_stats() {
        let mut s = AbyssState::new();
        let base_atk = s.hero_atk();
        s.upgrades[UpgradeKind::Sword.index()] = 5;
        assert_eq!(s.hero_atk(), base_atk + 10);

        let base_hp = s.hero_max_hp();
        s.upgrades[UpgradeKind::Vitality.index()] = 3;
        assert_eq!(s.hero_max_hp(), base_hp + 30);
    }

    #[test]
    fn crit_capped_at_60() {
        let mut s = AbyssState::new();
        s.upgrades[UpgradeKind::Crit.index()] = 200;
        assert!((s.hero_crit_rate() - 0.60).abs() < 1e-9);
    }

    #[test]
    fn upgrade_cost_grows() {
        let mut s = AbyssState::new();
        let c0 = s.upgrade_cost(UpgradeKind::Sword);
        s.upgrades[UpgradeKind::Sword.index()] = 5;
        let c5 = s.upgrade_cost(UpgradeKind::Sword);
        assert!(c5 > c0);
    }

    #[test]
    fn enemies_per_floor_const() {
        assert_eq!(enemies_per_floor(), 8);
    }

    #[test]
    fn log_truncates() {
        let mut s = AbyssState::new();
        for i in 0..120 {
            s.add_log(format!("msg {i}"));
        }
        assert!(s.log.len() <= 50);
    }
}
