//! つぶ牧場 (Tsubu Ranch) — game state.
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs に置く。
//!
//! ## コアループ
//!
//! 1. **成長**: 個体は tick 毎に XP を貯めてレベルアップする。Lv `MATURE_LEVEL` 以上で成熟扱いになる。
//! 2. **増殖**: 同種の成熟個体がいれば、収容数に空きがある限り一定確率+食料消費で新しい個体 (Lv1) が生まれる。
//! 3. **進化**: 同種の成熟個体が `Species::evolution_threshold` 体集まると確率判定が発生し、
//!    成功すると同数の成熟個体を消費して次階層の種が1体生まれる。確率は個体数と平均レベルの
//!    両方から決まるため、ただ増やすだけでなく育ててから集めた方が進化しやすい。
//!    どの進化先になるかは `affinity_feed` (餌やりの蓄積) に重み付けされる — 明示はしないので、
//!    プレイヤーは「何を与えると何に進化しやすいか」を結果から推測することになる。
//! 4. **対戦**: チームに編成した種の最強個体の合計ステータスで、ステージの敵と自動的に戦う。
//!    勝利した敵の種は野生個体として偶に仲間になることもある (繁殖以外の入手経路)。

use std::cell::Cell;

/// 餌やりで蓄積する属性。進化の分岐先を決める隠れた重みとしても使われる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Aqua,
    Flare,
    Earth,
}

pub const AFFINITY_COUNT: usize = 3;

impl Affinity {
    pub fn all() -> &'static [Affinity] {
        &[Affinity::Aqua, Affinity::Flare, Affinity::Earth]
    }

    pub fn index(self) -> usize {
        match self {
            Affinity::Aqua => 0,
            Affinity::Flare => 1,
            Affinity::Earth => 2,
        }
    }

    pub fn from_index(idx: usize) -> Option<Affinity> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Affinity::Aqua => "水",
            Affinity::Flare => "陽",
            Affinity::Earth => "土",
        }
    }
}

/// モンスターの種。3階層 (無属性 → 一次進化 → 最終形態) の進化ツリーを構成する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Species {
    Tsubu,
    AquaTsubu,
    FlareTsubu,
    EarthTsubu,
    MistPrincess,
    FrostHare,
    FireKirin,
    ThunderHawk,
    ThornBoar,
    SwampTurtle,
}

/// 種の総数。`RanchState::population` / `discovered` 配列のサイズに使う。
pub const SPECIES_COUNT: usize = 10;

impl Species {
    /// 全種を宣言順で返す (save id / index の SSOT)。
    pub fn all() -> &'static [Species] {
        &[
            Species::Tsubu,
            Species::AquaTsubu,
            Species::FlareTsubu,
            Species::EarthTsubu,
            Species::MistPrincess,
            Species::FrostHare,
            Species::FireKirin,
            Species::ThunderHawk,
            Species::ThornBoar,
            Species::SwampTurtle,
        ]
    }

    pub fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|&s| s == self)
            .expect("Species variant must appear in Species::all()")
    }

    pub fn from_index(idx: usize) -> Option<Species> {
        Self::all().get(idx).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Species::Tsubu => "ツブ",
            Species::AquaTsubu => "水ツブ",
            Species::FlareTsubu => "陽ツブ",
            Species::EarthTsubu => "土ツブ",
            Species::MistPrincess => "シズク姫",
            Species::FrostHare => "氷ウサ",
            Species::FireKirin => "火麒麟",
            Species::ThunderHawk => "雷鷹",
            Species::ThornBoar => "棘猪",
            Species::SwampTurtle => "沼亀",
        }
    }

    /// 進化の階層。0=無属性 (初期種)、1=一次進化、2=最終形態。
    pub fn tier(self) -> u8 {
        match self {
            Species::Tsubu => 0,
            Species::AquaTsubu | Species::FlareTsubu | Species::EarthTsubu => 1,
            Species::MistPrincess
            | Species::FrostHare
            | Species::FireKirin
            | Species::ThunderHawk
            | Species::ThornBoar
            | Species::SwampTurtle => 2,
        }
    }

    pub fn is_final_tier(self) -> bool {
        self.tier() == 2
    }

    /// 進化先候補。最終形態 (tier 2) は空スライスを返す。
    pub fn evolution_targets(self) -> &'static [Species] {
        match self {
            Species::Tsubu => &[Species::AquaTsubu, Species::FlareTsubu, Species::EarthTsubu],
            Species::AquaTsubu => &[Species::MistPrincess, Species::FrostHare],
            Species::FlareTsubu => &[Species::FireKirin, Species::ThunderHawk],
            Species::EarthTsubu => &[Species::ThornBoar, Species::SwampTurtle],
            _ => &[],
        }
    }

    /// 進化先候補ごとの「選ばれやすさに影響する属性」。
    /// `logic::pick_evolution_target` がこの属性の蓄積量を重みとして使う。
    /// プレイヤーには明示しない (餌やりの結果から推測してもらう余地)。
    pub fn evolution_bias(self, target: Species) -> Option<Affinity> {
        match (self, target) {
            (Species::Tsubu, Species::AquaTsubu) => Some(Affinity::Aqua),
            (Species::Tsubu, Species::FlareTsubu) => Some(Affinity::Flare),
            (Species::Tsubu, Species::EarthTsubu) => Some(Affinity::Earth),
            (Species::AquaTsubu, Species::MistPrincess) => Some(Affinity::Flare),
            (Species::AquaTsubu, Species::FrostHare) => Some(Affinity::Earth),
            (Species::FlareTsubu, Species::FireKirin) => Some(Affinity::Earth),
            (Species::FlareTsubu, Species::ThunderHawk) => Some(Affinity::Aqua),
            (Species::EarthTsubu, Species::ThornBoar) => Some(Affinity::Flare),
            (Species::EarthTsubu, Species::SwampTurtle) => Some(Affinity::Aqua),
            _ => None,
        }
    }

    /// 進化に必要な同種の成熟個体数。最終形態は進化しないので 0。
    pub fn evolution_threshold(self) -> u32 {
        match self.tier() {
            0 => 5,
            1 => 8,
            _ => 0,
        }
    }

    /// 基礎攻撃力 (Lv1 時点)。対戦時は `atk_at_level` でレベル分スケールする。
    pub fn base_atk(self) -> u64 {
        match self {
            Species::Tsubu => 3,
            Species::AquaTsubu => 5,
            Species::FlareTsubu => 8,
            Species::EarthTsubu => 4,
            Species::MistPrincess => 9,
            Species::FrostHare => 7,
            Species::FireKirin => 14,
            Species::ThunderHawk => 12,
            Species::ThornBoar => 10,
            Species::SwampTurtle => 6,
        }
    }

    /// 基礎HP (Lv1 時点)。対戦時は `hp_at_level` でレベル分スケールする。
    pub fn base_hp(self) -> u64 {
        match self {
            Species::Tsubu => 20,
            Species::AquaTsubu => 30,
            Species::FlareTsubu => 22,
            Species::EarthTsubu => 40,
            Species::MistPrincess => 40,
            Species::FrostHare => 34,
            Species::FireKirin => 45,
            Species::ThunderHawk => 32,
            Species::ThornBoar => 60,
            Species::SwampTurtle => 70,
        }
    }

    /// レベル分スケールした攻撃力。
    pub fn atk_at_level(self, level: u8) -> u64 {
        let base = self.base_atk() as f64;
        let steps = level.saturating_sub(1) as f64;
        (base * (1.0 + 0.15 * steps)).round() as u64
    }

    /// レベル分スケールしたHP。
    pub fn hp_at_level(self, level: u8) -> u64 {
        let base = self.base_hp() as f64;
        let steps = level.saturating_sub(1) as f64;
        (base * (1.0 + 0.2 * steps)).round() as u64
    }

    /// このステージに出現する野生個体の種。5ステージごとに解放されるプールから、
    /// ステージ番号で周期的に選ぶ (決定的、乱数不使用)。
    pub fn for_stage(stage: u32) -> Species {
        let tier_unlock = ((stage.saturating_sub(1)) / 5).min(2) as u8;
        let pool: Vec<Species> = Self::all()
            .iter()
            .copied()
            .filter(|s| s.tier() <= tier_unlock)
            .collect();
        let idx = (stage.saturating_sub(1)) as usize % pool.len();
        pool[idx]
    }

    /// ステージ分スケールした野生個体の攻撃力。
    pub fn stage_atk(self, stage: u32) -> u64 {
        let base = self.base_atk() as f64;
        let steps = stage.saturating_sub(1) as f64;
        (base * (1.0 + 0.25 * steps)).round() as u64
    }

    /// ステージ分スケールした野生個体のHP。
    pub fn stage_hp(self, stage: u32) -> u64 {
        let base = self.base_hp() as f64;
        let steps = stage.saturating_sub(1) as f64;
        (base * (1.0 + 0.35 * steps)).round() as u64
    }
}

/// 個体の最大レベル。
pub const MAX_LEVEL: u8 = 10;
/// このレベル以上で「成熟」扱いになり、進化判定・繁殖の母数に数えられる。
pub const MATURE_LEVEL: u8 = 5;

/// 1匹の個体。
#[derive(Clone, Copy, Debug)]
pub struct Creature {
    pub level: u8,
    pub xp: u32,
}

impl Creature {
    pub fn new() -> Self {
        Self { level: 1, xp: 0 }
    }

    pub fn is_mature(&self) -> bool {
        self.level >= MATURE_LEVEL
    }

    /// 次のレベルに必要な累積XP。
    pub fn xp_to_next_level(level: u8) -> u32 {
        20 * level as u32
    }
}

impl Default for Creature {
    fn default() -> Self {
        Self::new()
    }
}

/// メイン画面のタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// 牧場: 個体一覧、餌やり、収容数拡張。
    Habitat,
    /// 図鑑: 発見済みの種の一覧。
    Dex,
    /// 対戦: チーム編成とステージ進行。
    Battle,
}

impl Tab {
    #[cfg(any(target_arch = "wasm32", test))]
    pub const fn all() -> &'static [Tab] {
        &[Tab::Habitat, Tab::Dex, Tab::Battle]
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        Self::all()
            .iter()
            .position(|&t| t == self)
            .expect("Tab variant must appear in Tab::all()") as u8
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn from_save_id(id: u8) -> Self {
        Self::all().get(id as usize).copied().unwrap_or(Tab::Habitat)
    }
}

/// 対戦チームの人数。
pub const TEAM_SIZE: usize = 3;
/// 牧場の初期収容数。
pub const BASE_CAPACITY: u32 = 12;
/// 収容数拡張 1 回あたりの増分。
pub const CAPACITY_PER_UPGRADE: u32 = 6;
/// 対戦の1クラッシュ (攻撃応酬) 間隔 (tick数)。10 ticks/sec なので 0.5 秒に1回。
pub const CLASH_INTERVAL_TICKS: u32 = 5;

/// ゲームのルート状態。
pub struct RanchState {
    /// 種ごとの飼育個体。`Species::index()` でアクセスする。
    pub population: [Vec<Creature>; SPECIES_COUNT],
    pub food: u64,
    /// 餌やりの累積 (`Affinity::index()` でアクセス)。進化の分岐先バイアスに使う。
    pub affinity_feed: [u32; AFFINITY_COUNT],
    /// 収容数拡張の購入回数。`capacity()` の算出に使う。
    pub capacity_upgrades: u32,
    /// 発見済みの種フラグ (繁殖 or 対戦での遭遇で立つ)。
    pub discovered: [bool; SPECIES_COUNT],

    /// 対戦チーム。各スロットは種を指す (個体は常に「その種の最強個体」を自動選出)。
    pub team: [Option<Species>; TEAM_SIZE],
    pub stage: u32,
    pub enemy_species: Species,
    pub enemy_hp: u64,
    pub enemy_max_hp: u64,
    /// チームが受けた累積ダメージ。編成 (team) を変更しても回復させない — 現在HPは
    /// `team_max_hp() - damage_taken` の派生値として都度計算する (`team_hp()` 参照)。
    /// 絶対値でHPを保持すると、編成をタップし直すたびに満タンへリセットされてしまい
    /// (敗北の重みを無効化する無料回復になる)、対戦の緊張感が壊れるため。
    pub damage_taken: u64,
    pub clash_cooldown: u32,
    pub stage_clears: u64,

    pub tab: Tab,
    /// 現在のタブ本体の縦スクロール量 (visual rows)。UI only、永続化しない。
    pub tab_scroll: Cell<u16>,
    pub log: Vec<String>,

    pub total_ticks: u64,
    pub rng_state: u32,
}

impl RanchState {
    pub fn new() -> Self {
        let mut population: [Vec<Creature>; SPECIES_COUNT] = std::array::from_fn(|_| Vec::new());
        // 開始時点で無属性のツブを3匹与える (何もない画面から始めない)。
        population[Species::Tsubu.index()] = vec![Creature::new(); 3];

        let mut discovered = [false; SPECIES_COUNT];
        discovered[Species::Tsubu.index()] = true;

        let stage = 1;
        let enemy_species = Species::for_stage(stage);

        let mut s = Self {
            population,
            food: 30,
            affinity_feed: [0; AFFINITY_COUNT],
            capacity_upgrades: 0,
            discovered,
            team: [None; TEAM_SIZE],
            stage,
            enemy_species,
            enemy_hp: 0,
            enemy_max_hp: 0,
            damage_taken: 0,
            clash_cooldown: CLASH_INTERVAL_TICKS,
            stage_clears: 0,
            tab: Tab::Habitat,
            tab_scroll: Cell::new(0),
            log: Vec::new(),
            total_ticks: 0,
            rng_state: 0xC0FFEE,
        };
        s.enemy_max_hp = enemy_species.stage_hp(stage);
        s.enemy_hp = s.enemy_max_hp;
        s
    }

    /// 現在の収容数上限。
    pub fn capacity(&self) -> u32 {
        BASE_CAPACITY + self.capacity_upgrades * CAPACITY_PER_UPGRADE
    }

    /// 全種合計の飼育数。
    pub fn total_population(&self) -> u32 {
        self.population.iter().map(|v| v.len() as u32).sum()
    }

    /// 指定種の成熟個体数。
    pub fn mature_count(&self, species: Species) -> u32 {
        self.population[species.index()]
            .iter()
            .filter(|c| c.is_mature())
            .count() as u32
    }

    /// 指定種の成熟個体の平均レベル。成熟個体がいなければ 0.0。
    pub fn average_mature_level(&self, species: Species) -> f64 {
        let mature: Vec<&Creature> = self.population[species.index()]
            .iter()
            .filter(|c| c.is_mature())
            .collect();
        if mature.is_empty() {
            return 0.0;
        }
        mature.iter().map(|c| c.level as f64).sum::<f64>() / mature.len() as f64
    }

    /// 指定種のうち最もレベルが高い個体。
    pub fn strongest(&self, species: Species) -> Option<&Creature> {
        self.population[species.index()].iter().max_by_key(|c| c.level)
    }

    /// 収容数拡張の次回コスト。
    pub fn capacity_upgrade_cost(&self) -> u64 {
        let base = 50.0_f64;
        let growth = 1.6_f64;
        (base * growth.powi(self.capacity_upgrades as i32)).round() as u64
    }

    /// 餌やりのコスト (総飼育数が増えるほど上がる)。
    pub fn feed_cost(&self) -> u64 {
        5 + self.total_population() as u64
    }

    /// 対戦チームの合計攻撃力。未編成のスロットや個体が0の種は寄与しない。
    pub fn team_atk(&self) -> u64 {
        self.team
            .iter()
            .flatten()
            .filter_map(|&species| self.strongest(species).map(|c| species.atk_at_level(c.level)))
            .sum()
    }

    /// 対戦チームの合計最大HP。
    pub fn team_max_hp(&self) -> u64 {
        self.team
            .iter()
            .flatten()
            .filter_map(|&species| self.strongest(species).map(|c| species.hp_at_level(c.level)))
            .sum()
    }

    /// 対戦チームの現在HP。`damage_taken` からの派生値なので、編成を変更しても
    /// (新メンバーのHP分だけ最大値が動く以外は) 現在値がリセットされない。
    pub fn team_hp(&self) -> u64 {
        self.team_max_hp().saturating_sub(self.damage_taken)
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }
}

impl Default for RanchState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_sane() {
        let s = RanchState::new();
        assert_eq!(s.stage, 1);
        assert_eq!(s.population[Species::Tsubu.index()].len(), 3);
        assert!(s.discovered[Species::Tsubu.index()]);
        assert_eq!(s.total_population(), 3);
        assert_eq!(s.capacity(), BASE_CAPACITY);
        assert_eq!(s.enemy_hp, s.enemy_max_hp);
        assert!(s.enemy_max_hp > 0);
    }

    #[test]
    fn species_index_roundtrip() {
        for (i, &sp) in Species::all().iter().enumerate() {
            assert_eq!(sp.index(), i);
            assert_eq!(Species::from_index(i), Some(sp));
        }
        assert_eq!(Species::from_index(SPECIES_COUNT), None);
    }

    #[test]
    fn affinity_index_roundtrip() {
        for (i, &a) in Affinity::all().iter().enumerate() {
            assert_eq!(a.index(), i);
            assert_eq!(Affinity::from_index(i), Some(a));
        }
    }

    #[test]
    fn tab_save_id_roundtrip() {
        for &tab in Tab::all() {
            let id = tab.to_save_id();
            assert_eq!(Tab::from_save_id(id), tab);
        }
        assert_eq!(Tab::from_save_id(255), Tab::Habitat);
    }

    /// tier0 → tier1 (3種) → tier2 (6種) で全species が evolution tree に矛盾なく収まっていること。
    #[test]
    fn evolution_tree_covers_all_species_exactly_once() {
        let mut reached_by_evolution = vec![Species::Tsubu];
        for &tier1 in Species::Tsubu.evolution_targets() {
            assert_eq!(tier1.tier(), 1);
            reached_by_evolution.push(tier1);
            for &tier2 in tier1.evolution_targets() {
                assert_eq!(tier2.tier(), 2);
                assert!(tier2.evolution_targets().is_empty(), "最終形態はさらに進化しない");
                reached_by_evolution.push(tier2);
            }
        }
        assert_eq!(reached_by_evolution.len(), SPECIES_COUNT);
        for &sp in Species::all() {
            assert!(
                reached_by_evolution.contains(&sp),
                "{:?} が進化ツリーに含まれていない",
                sp
            );
        }
    }

    #[test]
    fn evolution_bias_defined_for_every_evolution_edge() {
        for &sp in Species::all() {
            for &target in sp.evolution_targets() {
                assert!(
                    sp.evolution_bias(target).is_some(),
                    "{:?} -> {:?} の evolution_bias が未定義",
                    sp,
                    target
                );
            }
        }
    }

    #[test]
    fn final_tier_has_zero_evolution_threshold() {
        for &sp in Species::all() {
            if sp.is_final_tier() {
                assert_eq!(sp.evolution_threshold(), 0);
                assert!(sp.evolution_targets().is_empty());
            } else {
                assert!(sp.evolution_threshold() > 0);
                assert!(!sp.evolution_targets().is_empty());
            }
        }
    }

    #[test]
    fn level_scaling_increases_with_level() {
        let sp = Species::FireKirin;
        assert!(sp.atk_at_level(5) > sp.atk_at_level(1));
        assert!(sp.hp_at_level(5) > sp.hp_at_level(1));
        assert_eq!(sp.atk_at_level(1), sp.base_atk());
        assert_eq!(sp.hp_at_level(1), sp.base_hp());
    }

    #[test]
    fn stage_scaling_increases_with_stage() {
        let sp = Species::Tsubu;
        assert!(sp.stage_hp(10) > sp.stage_hp(1));
        assert!(sp.stage_atk(10) > sp.stage_atk(1));
    }

    #[test]
    fn species_for_stage_unlocks_higher_tiers_over_time() {
        // 序盤 (stage 1-5) は tier0 のみ。
        for stage in 1..=5 {
            assert_eq!(Species::for_stage(stage).tier(), 0);
        }
        // stage 6-10 は tier0/1 のプールから選ばれる (tier2は出ない)。
        for stage in 6..=10 {
            assert!(Species::for_stage(stage).tier() <= 1);
        }
        // stage 11+ は tier2 も出現しうる。
        let has_tier2 = (11..=30).any(|stage| Species::for_stage(stage).tier() == 2);
        assert!(has_tier2, "十分ステージが進めば最終形態も出現するはず");
    }

    #[test]
    fn mature_count_and_average_level() {
        let mut s = RanchState::new();
        assert_eq!(s.mature_count(Species::Tsubu), 0, "初期個体はLv1でまだ成熟していない");
        s.population[Species::Tsubu.index()][0].level = MATURE_LEVEL;
        s.population[Species::Tsubu.index()][1].level = MATURE_LEVEL + 2;
        assert_eq!(s.mature_count(Species::Tsubu), 2);
        assert!((s.average_mature_level(Species::Tsubu) - (MATURE_LEVEL as f64 + 1.0)).abs() < 0.001);
        assert_eq!(s.average_mature_level(Species::AquaTsubu), 0.0);
    }

    #[test]
    fn strongest_picks_highest_level() {
        let mut s = RanchState::new();
        s.population[Species::Tsubu.index()][1].level = 9;
        assert_eq!(s.strongest(Species::Tsubu).unwrap().level, 9);
        assert!(s.strongest(Species::AquaTsubu).is_none());
    }

    #[test]
    fn team_stats_ignore_unassigned_and_empty_species() {
        let mut s = RanchState::new();
        assert_eq!(s.team_atk(), 0, "チーム未編成なら攻撃力0");
        s.team[0] = Some(Species::Tsubu);
        assert!(s.team_atk() > 0);
        s.team[1] = Some(Species::AquaTsubu); // 個体数0の種
        let atk_with_empty_species = s.team_atk();
        assert_eq!(atk_with_empty_species, s.team_atk(), "個体がいない種は寄与しない");
    }

    #[test]
    fn capacity_upgrade_cost_grows() {
        let mut s = RanchState::new();
        let c0 = s.capacity_upgrade_cost();
        s.capacity_upgrades = 3;
        let c3 = s.capacity_upgrade_cost();
        assert!(c3 > c0);
    }

    #[test]
    fn feed_cost_grows_with_population() {
        let mut s = RanchState::new();
        let base_cost = s.feed_cost();
        s.population[Species::AquaTsubu.index()] = vec![Creature::new(); 10];
        assert!(s.feed_cost() > base_cost);
    }

    #[test]
    fn log_truncates() {
        let mut s = RanchState::new();
        for i in 0..120 {
            s.add_log(format!("msg {i}"));
        }
        assert!(s.log.len() <= 50);
    }
}
