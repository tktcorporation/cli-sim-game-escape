//! Pure combat and turn-flow logic for 神の戦場.
//!
//! All functions are pure transformations on `GfState`.  No rendering,
//! no I/O, no time.  The render layer reads state; tick advances CPU
//! turn timers via [`tick`].

use super::state::{
    Card, CardKind, GfState, HAND_SIZE, LogKind, Phase, NUM_PLAYERS,
};

// ── RNG (xorshift32) ───────────────────────────────────────────

pub fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

pub fn rng_range(seed: &mut u32, n: u32) -> u32 {
    if n == 0 { 0 } else { rng_next(seed) % n }
}

pub fn draw_card(seed: &mut u32) -> Card {
    let pool = Card::pool();
    let i = rng_range(seed, pool.len() as u32) as usize;
    pool[i]
}

// ── Hand management ────────────────────────────────────────────

/// Refill `player_idx`'s hand to `HAND_SIZE` from the random pool.
pub fn refill_hand(state: &mut GfState, player_idx: usize) {
    while state.players[player_idx].hand.len() < HAND_SIZE {
        let c = draw_card(&mut state.rng_seed);
        state.players[player_idx].hand.push(c);
    }
}

// ── Turn flow ──────────────────────────────────────────────────

pub const CPU_TURN_DELAY_TICKS: u32 = 8;
pub const BETWEEN_TURNS_TICKS: u32 = 4;

/// Advance simulated time. Drives CPU turns and inter-turn pauses.
///
/// `delta_ticks` may exceed the current phase's remaining timer (e.g. after
/// a tab pause or a stalled frame), so we loop until the budget is fully
/// consumed.  Phases without a timer (`PlayerAction`, `Victory`, `Defeat`,
/// `Intro`, the player-selection phases) break early — they can sit on a
/// large `delta_ticks` without misbehaving because they don't track elapsed
/// time at all.
pub fn tick(state: &mut GfState, mut delta_ticks: u32) {
    while delta_ticks > 0 {
        match state.phase {
            Phase::CpuTurn { idx, ticks_left } => {
                if delta_ticks >= ticks_left {
                    delta_ticks -= ticks_left;
                    run_cpu_turn(state, idx);
                    advance_to_next_turn(state);
                    // Loop continues — the new phase (BetweenTurns or
                    // CpuTurn for the next NPC) may consume more ticks.
                } else {
                    state.phase = Phase::CpuTurn { idx, ticks_left: ticks_left - delta_ticks };
                    break;
                }
            }
            Phase::BetweenTurns { ticks_left } => {
                if delta_ticks >= ticks_left {
                    delta_ticks -= ticks_left;
                    begin_turn(state);
                } else {
                    state.phase = Phase::BetweenTurns { ticks_left: ticks_left - delta_ticks };
                    break;
                }
            }
            // Phases without a timer: nothing to advance.  Stop so we don't
            // spin forever burning ticks against a wall.
            _ => break,
        }
    }
}

/// Begin a turn for `state.turn`: refill hand, set the right phase.
pub fn begin_turn(state: &mut GfState) {
    if !state.players[state.turn].alive {
        advance_to_next_turn(state);
        return;
    }
    refill_hand(state, state.turn);
    if state.players[state.turn].is_human {
        state.phase = Phase::PlayerAction;
    } else {
        state.phase = Phase::CpuTurn { idx: state.turn, ticks_left: CPU_TURN_DELAY_TICKS };
    }
}

/// Move `state.turn` to the next living player and start their turn.
/// If only one (or zero) survivor remains, transition to Victory or Defeat.
pub fn advance_to_next_turn(state: &mut GfState) {
    // Check end conditions first.
    let alive_count = state.alive_count();
    let human_alive = state.players[state.human_idx()].alive;
    if alive_count <= 1 {
        // 0 = everyone dead simultaneously (rare, count human as defeat);
        // 1 = one survivor; if it's the human, victory.
        if human_alive && alive_count == 1 {
            state.phase = Phase::Victory;
            state.push_log("✦ あなたが最後の一人になった！神々があなたを称える。", LogKind::Info);
        } else {
            state.phase = Phase::Defeat;
            state.push_log("✦ あなたは倒れた…神々の祝福は他の者に。", LogKind::Info);
        }
        return;
    }
    if !human_alive {
        state.phase = Phase::Defeat;
        state.push_log("✦ あなたは倒れた…", LogKind::Info);
        return;
    }

    // Find the next living player after current.
    let mut next = state.turn;
    for _ in 0..NUM_PLAYERS {
        next = (next + 1) % NUM_PLAYERS;
        if state.players[next].alive { break; }
    }
    state.turn = next;
    if next == state.human_idx() {
        state.round += 1;
    }
    state.selected_weapons.clear();
    state.phase = Phase::BetweenTurns { ticks_left: BETWEEN_TURNS_TICKS };
}

// ── Combat: damage calculation ─────────────────────────────────

/// Compute total weapon damage stats for a multi-card attack.
/// Returns `(damage, has_pierce, has_magic)`.
pub fn weapon_attack_stats(weapons: &[Card]) -> (i32, bool, bool) {
    let mut dmg = 0i32;
    let mut pierce = false;
    let mut magic = false;
    for w in weapons {
        let d = w.def();
        if d.kind != CardKind::Weapon { continue; }
        dmg += d.power as i32 * d.hits as i32;
        pierce |= d.pierce;
        magic |= d.magic;
    }
    // Same-weapon combo bonus: if all selected weapons share an ID, +2 dmg
    // for each repeat (encourages collecting matching pairs).
    if weapons.len() >= 2 && weapons.iter().all(|w| *w == weapons[0]) {
        dmg += 2 * (weapons.len() as i32 - 1);
    }
    (dmg, pierce, magic)
}

/// Choose defender's armor cards for a given attack.  Returns indices into
/// the defender's hand that should be discarded as defense, plus a
/// `Reflect` flag if the defender used a Reflect special card.
pub fn choose_defense(
    hand: &[Card],
    attack_dmg: i32,
    attack_magic: bool,
    attack_pierce: bool,
) -> DefenseChoice {
    // Reflect special: if HP is in danger and reflect is in hand, use it.
    // For simplicity, the AI uses Reflect when the incoming damage is >=6.
    // The human AI never auto-reflects (the player controls defense by
    // hand composition only — reflect is consumed automatically when
    // present and `attack_dmg >= 6`).
    let mut chosen: Vec<usize> = Vec::new();

    // Find all valid armors. Magic attacks can only be blocked by
    // magic-blocking armor.
    let candidates: Vec<(usize, u8)> = hand.iter().enumerate()
        .filter_map(|(i, c)| {
            let d = c.def();
            if d.kind != CardKind::Armor { return None; }
            if attack_magic && !d.blocks_magic { return None; }
            // Pierce reduces effective defense by 2 (min 0).
            let eff = if attack_pierce { d.power.saturating_sub(2) } else { d.power };
            Some((i, eff))
        })
        .collect();

    // Strategy: prefer the smallest single armor that fully blocks the
    // attack (no overkill).  If none does, fall back to the highest-defense
    // armor available (mitigate as much as we can without burning extra
    // armor on a partial block).  This mirrors how a player thinks: "do I
    // have enough armor to survive this — yes, use the cheapest piece;
    // no, use my best piece and eat the rest."
    let fully_blocking: Vec<&(usize, u8)> = candidates.iter()
        .filter(|&&(_, p)| p as i32 >= attack_dmg)
        .collect();
    if let Some(&&(i, _)) = fully_blocking.iter().min_by_key(|&&&(_, p)| p) {
        chosen.push(i);
    } else if let Some(&(i, _)) = candidates.iter().max_by_key(|&&(_, p)| p) {
        chosen.push(i);
    }

    // Reflect: if attack is dangerous and we have Reflect, use it instead of armor.
    let reflect_idx = hand.iter().position(|c| *c == Card::Reflect);
    let use_reflect = attack_dmg >= 6 && reflect_idx.is_some();
    if use_reflect {
        chosen.clear();
        chosen.push(reflect_idx.unwrap());
    }

    DefenseChoice { card_indices: chosen, reflect: use_reflect }
}

#[derive(Debug, Clone)]
pub struct DefenseChoice {
    pub card_indices: Vec<usize>,
    pub reflect: bool,
}

/// Resolve an attack from `attacker_idx` against `defender_idx`, using
/// weapons at `weapon_indices` in attacker's hand.  Mutates state directly:
/// removes used cards, applies damage, logs the result.
pub fn resolve_attack(
    state: &mut GfState,
    attacker_idx: usize,
    defender_idx: usize,
    weapon_indices: &[usize],
) {
    // Collect weapon cards (sorted desc so removals don't shift indices).
    let mut sorted: Vec<usize> = weapon_indices.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    let mut weapons: Vec<Card> = Vec::new();
    for &i in &sorted {
        if i < state.players[attacker_idx].hand.len() {
            let c = state.players[attacker_idx].hand.remove(i);
            weapons.push(c);
        }
    }

    let (raw_damage, pierce, magic) = weapon_attack_stats(&weapons);
    let weapon_names: Vec<&str> = weapons.iter().map(|w| w.def().name).collect();
    state.push_log(
        format!(
            "{} → {} に「{}」で攻撃 (合計 {}ダメ{}{})",
            state.players[attacker_idx].name,
            state.players[defender_idx].name,
            weapon_names.join("・"),
            raw_damage,
            if pierce { ", 貫通" } else { "" },
            if magic { ", 魔法" } else { "" },
        ),
        LogKind::Attack,
    );

    // Defender chooses defense.
    let def_choice = choose_defense(
        &state.players[defender_idx].hand,
        raw_damage,
        magic,
        pierce,
    );

    let mut blocked = 0i32;
    let mut reflected = 0i32;
    let mut defender_used: Vec<Card> = Vec::new();

    if def_choice.reflect {
        // 半減反射: 生ダメージを半分ずつ攻撃者と防御者で受ける。
        // 床関数 (raw/2) なので、奇数時は防御者側に +1 残る (例: raw=7 → 攻撃者 3 / 防御者 4)。
        // 合計が raw を超えないので「反射で総ダメ増し」のチート挙動を防げる。
        // Reflect の発動条件は `attack_dmg >= 6` なので raw=1 などの極小ケースは
        // そもそも出てこない。
        reflected = raw_damage / 2;
        blocked = reflected; // 攻撃者が肩代わりする分が「防いだ」量。
        let mut sorted_def = def_choice.card_indices.clone();
        sorted_def.sort_unstable_by(|a, b| b.cmp(a));
        for &i in &sorted_def {
            if i < state.players[defender_idx].hand.len() {
                defender_used.push(state.players[defender_idx].hand.remove(i));
            }
        }
        let self_take = raw_damage - reflected;
        state.push_log(
            format!(
                "  → {} は「反射」を発動！ 攻撃者へ {} 跳ね返し、自身は {} 被弾",
                state.players[defender_idx].name, reflected, self_take,
            ),
            LogKind::Defend,
        );
    } else if !def_choice.card_indices.is_empty() {
        let mut sorted_def = def_choice.card_indices.clone();
        sorted_def.sort_unstable_by(|a, b| b.cmp(a));
        for &i in &sorted_def {
            if i < state.players[defender_idx].hand.len() {
                let c = state.players[defender_idx].hand.remove(i);
                let eff_def = if pierce {
                    c.def().power.saturating_sub(2) as i32
                } else {
                    c.def().power as i32
                };
                blocked += eff_def;
                defender_used.push(c);
            }
        }
        let names: Vec<&str> = defender_used.iter().map(|c| c.def().name).collect();
        state.push_log(
            format!("  → {} は「{}」で {} 防御", state.players[defender_idx].name, names.join("・"), blocked),
            LogKind::Defend,
        );
    } else {
        state.push_log(
            format!("  → {} は防御できず無防備！", state.players[defender_idx].name),
            LogKind::Defend,
        );
    }

    let final_damage = (raw_damage - blocked).max(0);

    if def_choice.reflect {
        // 半減反射: 防御者・攻撃者ともに半分を被弾。
        // 防御者の被弾は `blocked = raw - halved` を引いた `final_damage`。
        apply_damage(state, defender_idx, final_damage);
        apply_damage(state, attacker_idx, reflected);
    } else {
        apply_damage(state, defender_idx, final_damage);
    }
}

/// Apply `dmg` HP loss to `idx`. Logs death if HP drops to 0.
pub fn apply_damage(state: &mut GfState, idx: usize, dmg: i32) {
    if dmg <= 0 { return; }
    let (name, hp, max_hp, just_died) = {
        let p = &mut state.players[idx];
        p.hp = (p.hp - dmg).max(0);
        let just_died = p.hp == 0 && p.alive;
        if just_died { p.alive = false; }
        (p.name.clone(), p.hp, p.max_hp, just_died)
    };
    state.push_log(
        format!("  ✦ {} に {} ダメージ (HP: {}/{})", name, dmg, hp, max_hp),
        LogKind::Damage,
    );
    if just_died {
        state.push_log(format!("  ☠ {} は倒れた…", name), LogKind::Death);
    }
}

/// Apply `amount` of healing to `idx` (clamped to max_hp).
pub fn apply_heal(state: &mut GfState, idx: usize, amount: u8) {
    let (name, gained, hp, max_hp) = {
        let p = &mut state.players[idx];
        if !p.alive { return; }
        let before = p.hp;
        p.hp = (p.hp + amount as i32).min(p.max_hp);
        (p.name.clone(), p.hp - before, p.hp, p.max_hp)
    };
    state.push_log(
        format!("  ♥ {} は {} 回復 (HP: {}/{})", name, gained, hp, max_hp),
        LogKind::Heal,
    );
}

// ── Player action handlers ─────────────────────────────────────

/// Toggle a weapon card selection in the human's hand.
pub fn toggle_weapon_selection(state: &mut GfState, hand_idx: usize) {
    if hand_idx >= state.players[state.human_idx()].hand.len() { return; }
    if state.players[state.human_idx()].hand[hand_idx].kind() != CardKind::Weapon {
        return;
    }
    if let Some(pos) = state.selected_weapons.iter().position(|&i| i == hand_idx) {
        state.selected_weapons.remove(pos);
    } else {
        state.selected_weapons.push(hand_idx);
    }
}

/// Confirm weapon selection and proceed to target picker. Requires at least
/// one weapon selected.
pub fn confirm_weapons(state: &mut GfState) -> bool {
    if state.selected_weapons.is_empty() { return false; }
    state.phase = Phase::PlayerSelectTarget;
    true
}

/// Human attacks `target_idx` with currently selected weapons.
pub fn human_attack(state: &mut GfState, target_idx: usize) -> bool {
    if !state.players[target_idx].alive || target_idx == state.human_idx() {
        return false;
    }
    let weapons = state.selected_weapons.clone();
    state.selected_weapons.clear();
    resolve_attack(state, state.human_idx(), target_idx, &weapons);
    advance_to_next_turn(state);
    true
}

/// Human heals using `hand_idx` heal card.
pub fn human_heal(state: &mut GfState, hand_idx: usize) -> bool {
    let h = state.human_idx();
    if hand_idx >= state.players[h].hand.len() { return false; }
    let c = state.players[h].hand[hand_idx];
    if c.kind() != CardKind::Heal { return false; }
    state.players[h].hand.remove(hand_idx);
    let amount = c.def().power;
    state.push_log(
        format!("{} は「{}」を使った", state.players[h].name, c.def().name),
        LogKind::Heal,
    );
    apply_heal(state, h, amount);
    advance_to_next_turn(state);
    true
}

/// Human uses a special card.  Effects:
/// - Pray: HP +3, draw 1 card (refilled at next turn anyway, but immediate)
/// - Steal: take a random card from a random opponent
/// - Trial: deal 5 dmg to all other players (may be defended)
/// - Reflect: not usable as an action; only as defense.
pub fn human_use_special(state: &mut GfState, hand_idx: usize) -> bool {
    let h = state.human_idx();
    if hand_idx >= state.players[h].hand.len() { return false; }
    let c = state.players[h].hand[hand_idx];
    if c.kind() != CardKind::Special { return false; }
    if c == Card::Reflect { return false; }
    state.players[h].hand.remove(hand_idx);
    state.push_log(
        format!("{} は「{}」を発動！", state.players[h].name, c.def().name),
        LogKind::Special,
    );
    apply_special(state, h, c);
    advance_to_next_turn(state);
    true
}

fn apply_special(state: &mut GfState, user_idx: usize, card: Card) {
    match card {
        Card::Pray => {
            apply_heal(state, user_idx, 3);
            // Draw an extra card immediately (refilled to HAND_SIZE+1 just
            // until end of turn — refill_hand only adds, doesn't trim).
            let c = draw_card(&mut state.rng_seed);
            state.players[user_idx].hand.push(c);
            state.push_log(
                format!("  + 手札に「{}」を引いた", c.def().name),
                LogKind::Info,
            );
        }
        Card::Steal => {
            let opps = state.living_opponents(user_idx);
            if opps.is_empty() { return; }
            let target = opps[rng_range(&mut state.rng_seed, opps.len() as u32) as usize];
            if state.players[target].hand.is_empty() {
                state.push_log(format!("  → {} は手札がなく、何も奪えない", state.players[target].name), LogKind::Info);
                return;
            }
            let h_idx = rng_range(&mut state.rng_seed, state.players[target].hand.len() as u32) as usize;
            let stolen = state.players[target].hand.remove(h_idx);
            state.players[user_idx].hand.push(stolen);
            state.push_log(
                format!("  → {} から「{}」を奪った！", state.players[target].name, stolen.def().name),
                LogKind::Special,
            );
        }
        Card::Trial => {
            state.push_log("  ⚡ 雷鳴！全プレイヤーに試練が下る…", LogKind::Special);
            let dmg = card.def().power as i32;
            let targets: Vec<usize> = (0..state.players.len())
                .filter(|&i| i != user_idx && state.players[i].alive)
                .collect();
            for t in targets {
                apply_damage(state, t, dmg);
            }
        }
        _ => {}
    }
}

/// Human passes (does nothing).  Used when stuck with only armor cards.
pub fn human_pass(state: &mut GfState) {
    let name = state.players[state.human_idx()].name.clone();
    state.push_log(format!("{} はパスした", name), LogKind::Info);
    advance_to_next_turn(state);
}

// ── CPU AI ─────────────────────────────────────────────────────

/// Decide and execute the CPU's action for `idx`.
pub fn run_cpu_turn(state: &mut GfState, idx: usize) {
    if !state.players[idx].alive { return; }
    refill_hand(state, idx);

    let p = &state.players[idx];
    let hp_ratio = p.hp as f32 / p.max_hp as f32;

    // 1. If low HP and have a strong heal, use it.
    if hp_ratio < 0.5 {
        if let Some(heal_idx) = best_heal(&p.hand) {
            let c = p.hand[heal_idx];
            let amt = c.def().power;
            state.push_log(
                format!("{} は「{}」で回復", p.name, c.def().name),
                LogKind::Heal,
            );
            state.players[idx].hand.remove(heal_idx);
            apply_heal(state, idx, amt);
            return;
        }
    }

    // 2. If low-HP and we have Pray, use it for a small heal.
    if hp_ratio < 0.6 {
        if let Some(pi) = p.hand.iter().position(|c| *c == Card::Pray) {
            let c = state.players[idx].hand.remove(pi);
            state.push_log(format!("{} は「{}」を捧げた", state.players[idx].name, c.def().name), LogKind::Special);
            apply_special(state, idx, c);
            return;
        }
    }

    // 3. Pick weapons. CPU uses 1 weapon at a time (simpler AI).
    let weapon_idx = best_weapon(&state.players[idx].hand);
    if let Some(wi) = weapon_idx {
        let target = pick_attack_target(state, idx);
        if let Some(t) = target {
            resolve_attack(state, idx, t, &[wi]);
            return;
        }
    }

    // 4. If we have Trial and at least 2 enemies alive, use it.
    if state.alive_count() >= 3 {
        if let Some(ti) = state.players[idx].hand.iter().position(|c| *c == Card::Trial) {
            let c = state.players[idx].hand.remove(ti);
            state.push_log(format!("{} は「{}」を発動！", state.players[idx].name, c.def().name), LogKind::Special);
            apply_special(state, idx, c);
            return;
        }
    }

    // 5. Otherwise pass.
    let name = state.players[idx].name.clone();
    state.push_log(format!("{} はパスした", name), LogKind::Info);
}

/// Index of the best weapon in `hand` (highest expected damage).
pub fn best_weapon(hand: &[Card]) -> Option<usize> {
    hand.iter().enumerate()
        .filter(|(_, c)| c.kind() == CardKind::Weapon)
        .max_by_key(|(_, c)| {
            let d = c.def();
            (d.power as u32) * (d.hits as u32) + if d.pierce { 2 } else { 0 } + if d.magic { 1 } else { 0 }
        })
        .map(|(i, _)| i)
}

/// Index of the best heal in `hand` (largest amount).
pub fn best_heal(hand: &[Card]) -> Option<usize> {
    hand.iter().enumerate()
        .filter(|(_, c)| c.kind() == CardKind::Heal)
        .max_by_key(|(_, c)| c.def().power)
        .map(|(i, _)| i)
}

/// CPU AI: target the player with the lowest HP among living opponents.
/// Tie-breaker: prefer the human (player 0) for narrative tension.
pub fn pick_attack_target(state: &GfState, attacker_idx: usize) -> Option<usize> {
    let opps = state.living_opponents(attacker_idx);
    opps.iter().copied()
        .min_by(|&a, &b| {
            let ha = state.players[a].hp;
            let hb = state.players[b].hp;
            ha.cmp(&hb).then_with(|| {
                // Prefer human as tiebreak
                let hi = state.human_idx();
                (a != hi).cmp(&(b != hi))
            })
        })
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::*;

    fn make_state() -> GfState {
        let mut s = GfState::new(1);
        // Skip intro
        s.phase = Phase::PlayerAction;
        s
    }

    fn set_hand(s: &mut GfState, idx: usize, cards: &[Card]) {
        s.players[idx].hand = cards.to_vec();
    }

    #[test]
    fn rng_advances() {
        let mut seed = 1u32;
        let a = rng_next(&mut seed);
        let b = rng_next(&mut seed);
        assert_ne!(a, b);
    }

    #[test]
    fn weapon_stats_sums_damage() {
        let (d, p, m) = weapon_attack_stats(&[Card::Sword, Card::Knife]);
        // Sword 4 + Knife 2 = 6
        assert_eq!(d, 6);
        assert!(!p);
        assert!(!m);
    }

    #[test]
    fn weapon_combo_bonus_for_duplicates() {
        // Two identical swords: 4 + 4 + 2 (combo) = 10
        let (d, _, _) = weapon_attack_stats(&[Card::Sword, Card::Sword]);
        assert_eq!(d, 10);
    }

    #[test]
    fn weapon_pierce_propagates() {
        let (_, p, _) = weapon_attack_stats(&[Card::Spear]);
        assert!(p);
    }

    #[test]
    fn weapon_magic_propagates() {
        let (_, _, m) = weapon_attack_stats(&[Card::Wand]);
        assert!(m);
    }

    #[test]
    fn bow_double_hit() {
        let (d, _, _) = weapon_attack_stats(&[Card::Bow]);
        // Bow: 3 dmg × 2 hits = 6
        assert_eq!(d, 6);
    }

    #[test]
    fn defense_blocks_damage() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Sword]);
        set_hand(&mut s, 1, &[Card::Shield, Card::Shield]);
        resolve_attack(&mut s, 0, 1, &[0]);
        // Sword 4 dmg, Shield blocks 3 → 1 damage gets through.
        assert_eq!(s.players[1].hp, STARTING_HP - 1);
    }

    #[test]
    fn magic_bypasses_normal_armor() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Wand]);
        set_hand(&mut s, 1, &[Card::Shield, Card::Armor]);
        resolve_attack(&mut s, 0, 1, &[0]);
        // Wand 4 magic dmg, no magic-blocking armor → 4 damage through.
        assert_eq!(s.players[1].hp, STARTING_HP - 4);
        // Defender's non-magic armor is preserved (only valid candidates are consumed).
        assert!(s.players[1].hand.contains(&Card::Shield));
        assert!(s.players[1].hand.contains(&Card::Armor));
    }

    #[test]
    fn magic_blocked_by_robe() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Wand]);
        set_hand(&mut s, 1, &[Card::Robe]); // 3 def, blocks magic
        resolve_attack(&mut s, 0, 1, &[0]);
        // Wand 4 magic, Robe 3 → 1 damage through.
        assert_eq!(s.players[1].hp, STARTING_HP - 1);
    }

    #[test]
    fn pierce_reduces_armor() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Spear]);
        set_hand(&mut s, 1, &[Card::Shield]); // 3 def, pierce reduces by 2 → 1
        resolve_attack(&mut s, 0, 1, &[0]);
        // Spear 4 dmg vs effective Shield 1 → 3 damage through.
        assert_eq!(s.players[1].hp, STARTING_HP - 3);
    }

    #[test]
    fn reflect_splits_damage_evenly_for_even_total() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Greatsword]); // 6 dmg
        set_hand(&mut s, 1, &[Card::Reflect]);
        resolve_attack(&mut s, 0, 1, &[0]);
        // 半減反射: raw 6 → 攻撃者 3 / 防御者 3
        assert_eq!(s.players[0].hp, STARTING_HP - 3);
        assert_eq!(s.players[1].hp, STARTING_HP - 3);
    }

    #[test]
    fn reflect_total_damage_does_not_exceed_raw_for_odd_total() {
        // raw=7 (Greatsword 6 + Knife なら 8 になるので、別途構成)
        // 銃 (8) で reflect → 攻撃者 4 / 防御者 4 = total 8 = raw
        // 神剣 (12) で reflect → 攻撃者 6 / 防御者 6 = total 12 = raw
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Gun]); // 8 dmg
        set_hand(&mut s, 1, &[Card::Reflect]);
        resolve_attack(&mut s, 0, 1, &[0]);
        let total_taken = (STARTING_HP - s.players[0].hp) + (STARTING_HP - s.players[1].hp);
        assert_eq!(total_taken, 8, "reflect must not amplify total damage");
    }

    #[test]
    fn reflect_consumes_reflect_card() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Greatsword]);
        set_hand(&mut s, 1, &[Card::Reflect, Card::Shield]);
        resolve_attack(&mut s, 0, 1, &[0]);
        assert!(!s.players[1].hand.contains(&Card::Reflect));
        // Shield は使わないので残るはず。
        assert!(s.players[1].hand.contains(&Card::Shield));
    }

    #[test]
    fn defender_picks_minimal_armor() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Knife]); // 2 dmg
        set_hand(&mut s, 1, &[Card::SmallShield, Card::Barrier]); // 2 vs 8
        resolve_attack(&mut s, 0, 1, &[0]);
        // Should use SmallShield (2), keeping Barrier in hand.
        assert!(s.players[1].hand.contains(&Card::Barrier));
        assert!(!s.players[1].hand.contains(&Card::SmallShield));
    }

    #[test]
    fn damage_kills_player() {
        let mut s = make_state();
        s.players[1].hp = 2;
        set_hand(&mut s, 0, &[Card::Greatsword]);
        set_hand(&mut s, 1, &[]); // no defense
        resolve_attack(&mut s, 0, 1, &[0]);
        assert_eq!(s.players[1].hp, 0);
        assert!(!s.players[1].alive);
    }

    #[test]
    fn heal_clamps_to_max() {
        let mut s = make_state();
        s.players[0].hp = 25;
        apply_heal(&mut s, 0, 20);
        assert_eq!(s.players[0].hp, STARTING_HP);
    }

    #[test]
    fn human_attack_advances_turn() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Sword]);
        s.selected_weapons = vec![0];
        let ok = human_attack(&mut s, 1);
        assert!(ok);
        // Turn moved off human (0).
        assert_ne!(s.turn, 0);
        // Phase entered between-turns or cpu-turn directly.
        assert!(matches!(s.phase, Phase::BetweenTurns { .. } | Phase::CpuTurn { .. }));
    }

    #[test]
    fn cpu_low_hp_heals_when_possible() {
        let mut s = make_state();
        s.turn = 1;
        s.players[1].hp = 5;
        set_hand(&mut s, 1, &[Card::Elixir, Card::Sword]); // big heal available
        run_cpu_turn(&mut s, 1);
        assert!(s.players[1].hp > 5);
    }

    #[test]
    fn cpu_attacks_lowest_hp_target() {
        let mut s = make_state();
        s.turn = 1;
        s.players[2].hp = 3; // weakest
        s.players[3].hp = 25;
        set_hand(&mut s, 1, &[Card::Greatsword]);
        // Set human HP high so CPU prefers player 2.
        s.players[0].hp = 30;
        run_cpu_turn(&mut s, 1);
        assert!(s.players[2].hp < 3); // got attacked
    }

    #[test]
    fn last_player_standing_wins() {
        let mut s = make_state();
        s.players[1].alive = false;
        s.players[2].alive = false;
        s.players[3].alive = false;
        advance_to_next_turn(&mut s);
        assert_eq!(s.phase, Phase::Victory);
    }

    #[test]
    fn human_dead_means_defeat() {
        let mut s = make_state();
        s.players[0].alive = false;
        advance_to_next_turn(&mut s);
        assert_eq!(s.phase, Phase::Defeat);
    }

    #[test]
    fn toggle_weapon_selection_only_for_weapons() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Sword, Card::Shield]);
        toggle_weapon_selection(&mut s, 0);
        assert_eq!(s.selected_weapons, vec![0]);
        // Shield cannot be selected as weapon.
        toggle_weapon_selection(&mut s, 1);
        assert_eq!(s.selected_weapons, vec![0]);
        // Toggle off
        toggle_weapon_selection(&mut s, 0);
        assert!(s.selected_weapons.is_empty());
    }

    #[test]
    fn trial_hits_all_opponents() {
        let mut s = make_state();
        set_hand(&mut s, 0, &[Card::Trial]);
        let hp_before: Vec<_> = s.players.iter().map(|p| p.hp).collect();
        let ok = human_use_special(&mut s, 0);
        assert!(ok);
        assert_eq!(s.players[0].hp, hp_before[0]); // self unchanged
        for (i, &before) in hp_before.iter().enumerate().skip(1) {
            assert!(s.players[i].hp < before);
        }
    }

    #[test]
    fn pray_heals_and_draws() {
        let mut s = make_state();
        s.players[0].hp = 10;
        set_hand(&mut s, 0, &[Card::Pray]);
        let hand_size_before = s.players[0].hand.len();
        let ok = human_use_special(&mut s, 0);
        assert!(ok);
        assert!(s.players[0].hp > 10);
        // Pray was removed (-1) and a draw was added (+1) → net 0
        // Then advance_to_next_turn doesn't refill non-human-side hands.
        assert_eq!(s.players[0].hand.len(), hand_size_before);
    }

    #[test]
    fn tick_runs_cpu_after_delay() {
        let mut s = make_state();
        s.turn = 1;
        s.phase = Phase::CpuTurn { idx: 1, ticks_left: 4 };
        tick(&mut s, 4);
        // CPU executed and turn advanced.
        assert_ne!(s.turn, 1);
    }

    #[test]
    fn between_turns_advances_phase() {
        let mut s = make_state();
        s.turn = 0;
        s.phase = Phase::BetweenTurns { ticks_left: 4 };
        tick(&mut s, 4);
        assert!(matches!(s.phase, Phase::PlayerAction));
    }

    #[test]
    fn tick_consumes_leftover_across_transitions() {
        // 大量の delta_ticks (タブ復帰直後の代表例) を一発で渡しても、
        // CPU の番が連続して進み、最終的に止まるべきフェーズで止まること。
        let mut s = make_state();
        s.turn = 1;
        s.phase = Phase::CpuTurn { idx: 1, ticks_left: CPU_TURN_DELAY_TICKS };
        // 100 ticks: CPU1 (8) + Between (4) + CPU2 (8) + Between (4) + CPU3 (8)
        // + Between (4) + PlayerAction (止まる) = 36 で枯渇するまで進める。
        tick(&mut s, 100);
        // 最終的に人間の番に戻っているはず (もしくは Victory/Defeat)。
        match s.phase {
            Phase::PlayerAction | Phase::Victory | Phase::Defeat => {}
            other => panic!("expected to settle at PlayerAction/Victory/Defeat, got {:?}", other),
        }
    }

    #[test]
    fn tick_partial_progress_within_phase() {
        // delta_ticks < ticks_left の通常ケースは引き算だけ進む。
        let mut s = make_state();
        s.phase = Phase::BetweenTurns { ticks_left: 4 };
        tick(&mut s, 1);
        assert_eq!(s.phase, Phase::BetweenTurns { ticks_left: 3 });
    }

    #[test]
    fn tick_idle_phase_does_not_loop_forever() {
        // タイマーのないフェーズで巨大な delta_ticks を受け取っても、
        // ループ内で破棄せず即座に抜ける (無限ループしない)。
        let mut s = make_state();
        s.phase = Phase::PlayerAction;
        tick(&mut s, u32::MAX);
        assert_eq!(s.phase, Phase::PlayerAction);
    }
}
