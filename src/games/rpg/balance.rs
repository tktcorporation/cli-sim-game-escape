//! Difficulty configuration — centralized knobs for tuning the dungeon.
//!
//! `BalanceConfig` exposes scalar multipliers applied at the few hot
//! decision points in `logic.rs` (enemy stats, rewards, trap damage). The
//! default preset (`standard`) leaves every multiplier at 1.0, so existing
//! behavior and tests are preserved unless a non-default preset is selected.
//!
//! The simulator (`sim.rs`) uses these to run automated playthroughs and
//! report whether a given config produces the difficulty curve we want.

/// Multiplicative knobs applied to the hand-tuned base values in `state.rs`.
///
/// Floor-aware multipliers (`*_per_floor`) are added on top of the flat
/// multiplier, so e.g. `enemy_hp_mul = 1.0` + `enemy_hp_per_floor = 0.05`
/// gives 1.0× on B1, 1.05× on B2, … 1.45× on B10.
///
/// `#[allow(dead_code)]` covers fields / preset constructors that are used
/// only by the `#[cfg(test)]`-gated simulator. They're left public so the
/// preset names stay part of the documented difficulty surface.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct BalanceConfig {
    pub name: &'static str,

    // ── Enemy ────────────────────────────────────────────────
    pub enemy_hp_mul: f32,
    pub enemy_atk_mul: f32,
    pub enemy_def_mul: f32,
    pub enemy_hp_per_floor: f32,
    pub enemy_atk_per_floor: f32,

    // ── Rewards ──────────────────────────────────────────────
    pub exp_mul: f32,
    pub gold_mul: f32,

    // ── Hazards ──────────────────────────────────────────────
    pub trap_damage_mul: f32,
    pub treasure_trap_chance_mul: f32,
}

impl Default for BalanceConfig {
    fn default() -> Self {
        Self::standard()
    }
}

#[allow(dead_code)]
impl BalanceConfig {
    /// Vanilla balance — multipliers all 1.0. Reproduces existing behavior.
    pub const fn standard() -> Self {
        Self {
            name: "standard",
            enemy_hp_mul: 1.0,
            enemy_atk_mul: 1.0,
            enemy_def_mul: 1.0,
            enemy_hp_per_floor: 0.0,
            enemy_atk_per_floor: 0.0,
            exp_mul: 1.0,
            gold_mul: 1.0,
            trap_damage_mul: 1.0,
            treasure_trap_chance_mul: 1.0,
        }
    }

    pub const fn easy() -> Self {
        Self {
            name: "easy",
            enemy_hp_mul: 0.75,
            enemy_atk_mul: 0.75,
            enemy_def_mul: 0.9,
            enemy_hp_per_floor: 0.0,
            enemy_atk_per_floor: 0.0,
            exp_mul: 1.25,
            gold_mul: 1.25,
            trap_damage_mul: 0.5,
            treasure_trap_chance_mul: 0.6,
        }
    }

    pub const fn hard() -> Self {
        Self {
            name: "hard",
            enemy_hp_mul: 1.25,
            enemy_atk_mul: 1.2,
            enemy_def_mul: 1.1,
            enemy_hp_per_floor: 0.03,
            enemy_atk_per_floor: 0.02,
            exp_mul: 0.9,
            gold_mul: 0.85,
            trap_damage_mul: 1.5,
            treasure_trap_chance_mul: 1.3,
        }
    }

    pub const fn brutal() -> Self {
        Self {
            name: "brutal",
            enemy_hp_mul: 1.5,
            enemy_atk_mul: 1.4,
            enemy_def_mul: 1.2,
            enemy_hp_per_floor: 0.05,
            enemy_atk_per_floor: 0.04,
            exp_mul: 0.8,
            gold_mul: 0.75,
            trap_damage_mul: 2.0,
            treasure_trap_chance_mul: 1.5,
        }
    }

    pub fn scale_enemy_hp(&self, base: u32, floor: u32) -> u32 {
        scale_u32(base, self.enemy_hp_mul + self.enemy_hp_per_floor * floor as f32)
    }

    pub fn scale_enemy_atk(&self, base: u32, floor: u32) -> u32 {
        scale_u32(base, self.enemy_atk_mul + self.enemy_atk_per_floor * floor as f32)
    }

    pub fn scale_enemy_def(&self, base: u32) -> u32 {
        scale_u32(base, self.enemy_def_mul)
    }

    pub fn scale_exp(&self, base: u32) -> u32 {
        scale_u32(base, self.exp_mul)
    }

    pub fn scale_gold(&self, base: u32) -> u32 {
        scale_u32(base, self.gold_mul)
    }

    pub fn scale_trap_damage(&self, base: u32) -> u32 {
        scale_u32(base, self.trap_damage_mul)
    }

    pub fn scale_treasure_trap_chance(&self, base_pct: u32) -> u32 {
        scale_u32(base_pct, self.treasure_trap_chance_mul).min(100)
    }
}

fn scale_u32(value: u32, mul: f32) -> u32 {
    if mul <= 0.0 {
        return 0;
    }
    let scaled = (value as f32 * mul).round();
    if scaled < 0.0 {
        0
    } else if scaled > u32::MAX as f32 {
        u32::MAX
    } else {
        scaled as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_is_identity() {
        let d = BalanceConfig::standard();
        assert_eq!(d.scale_enemy_hp(100, 5), 100);
        assert_eq!(d.scale_enemy_atk(20, 1), 20);
        assert_eq!(d.scale_enemy_def(8), 8);
        assert_eq!(d.scale_exp(15), 15);
        assert_eq!(d.scale_gold(20), 20);
        assert_eq!(d.scale_trap_damage(10), 10);
        assert_eq!(d.scale_treasure_trap_chance(35), 35);
    }

    #[test]
    fn hard_increases_enemy_stats() {
        let d = BalanceConfig::hard();
        assert!(d.scale_enemy_hp(100, 1) > 100);
        assert!(d.scale_enemy_atk(20, 5) > 20);
        assert!(d.scale_gold(100) < 100);
    }

    #[test]
    fn easy_decreases_enemy_stats() {
        let d = BalanceConfig::easy();
        assert!(d.scale_enemy_hp(100, 5) < 100);
        assert!(d.scale_enemy_atk(20, 5) < 20);
        assert!(d.scale_gold(100) > 100);
    }

    #[test]
    fn floor_scaling_compounds() {
        let d = BalanceConfig::hard();
        let f1 = d.scale_enemy_hp(100, 1);
        let f10 = d.scale_enemy_hp(100, 10);
        assert!(f10 > f1);
    }

    #[test]
    fn scale_clamps_negative_multiplier() {
        assert_eq!(scale_u32(100, -1.0), 0);
    }

    #[test]
    fn trap_chance_clamped_to_100() {
        let mut d = BalanceConfig::standard();
        d.treasure_trap_chance_mul = 5.0;
        assert_eq!(d.scale_treasure_trap_chance(50), 100);
    }
}
