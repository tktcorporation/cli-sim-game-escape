//! たまごっち風育成ゲームの state。

/// ペットの成長段階。`Dead` 以外は `age_ticks` の経過で進行する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 卵 (孵化前)。プレイヤーがタップすると Baby に遷移。
    Egg,
    Baby,
    Child,
    Teen,
    Adult,
    Elder,
    Dead,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Egg => "たまご",
            Stage::Baby => "ベビー",
            Stage::Child => "チャイルド",
            Stage::Teen => "ティーン",
            Stage::Adult => "アダルト",
            Stage::Elder => "シニア",
            Stage::Dead => "★天に召されました",
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        match self {
            Stage::Egg => 0,
            Stage::Baby => 1,
            Stage::Child => 2,
            Stage::Teen => 3,
            Stage::Adult => 4,
            Stage::Elder => 5,
            Stage::Dead => 6,
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn from_save_id(id: u8) -> Self {
        match id {
            0 => Stage::Egg,
            1 => Stage::Baby,
            2 => Stage::Child,
            3 => Stage::Teen,
            4 => Stage::Adult,
            5 => Stage::Elder,
            _ => Stage::Dead,
        }
    }
}

/// 到達した節目に応じて獲得する称号。世代をまたいで保持される。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Milestone {
    /// Child 到達
    Sprout,
    /// Teen 到達
    Rebel,
    /// Adult 到達
    FineAdult,
    /// Elder 到達
    LongLifeStar,
    /// 2 世代目以降でベスト寿命を更新
    Legend,
}

impl Milestone {
    /// 進行順 (易→難)。「つぎの目標」と最高位称号の判定はこの順序に依存する。
    pub const ALL: [Milestone; 5] = [
        Milestone::Sprout,
        Milestone::Rebel,
        Milestone::FineAdult,
        Milestone::LongLifeStar,
        Milestone::Legend,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Milestone::Sprout => "すくすく",
            Milestone::Rebel => "はんこうき",
            Milestone::FineAdult => "りっぱなおとな",
            Milestone::LongLifeStar => "ながいきのほし",
            Milestone::Legend => "でんせつ",
        }
    }

    /// 「つぎの目標」表示用の達成条件ヒント。
    pub fn goal_hint(self) -> &'static str {
        match self {
            Milestone::Sprout => "チャイルドまで そだてる",
            Milestone::Rebel => "ティーンまで そだてる",
            Milestone::FineAdult => "アダルトまで そだてる",
            Milestone::LongLifeStar => "シニアまで そだてる",
            Milestone::Legend => "ベスト寿命を こうしん",
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn to_save_id(self) -> u8 {
        match self {
            Milestone::Sprout => 0,
            Milestone::Rebel => 1,
            Milestone::FineAdult => 2,
            Milestone::LongLifeStar => 3,
            Milestone::Legend => 4,
        }
    }

    /// 未知の id は `None`。新しい称号を知る別バージョンの save を読んでも
    /// 既知の称号だけ残して安全にロードできる。
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn from_save_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Milestone::Sprout),
            1 => Some(Milestone::Rebel),
            2 => Some(Milestone::FineAdult),
            3 => Some(Milestone::LongLifeStar),
            4 => Some(Milestone::Legend),
            _ => None,
        }
    }
}

/// 直近のアクション。表情や演出に反映する短期 state。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastAction {
    Fed,
    Played,
    Bathed,
    Medicated,
    Petted,
    Slept,
    Refused,
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    /// 空腹度 (満腹=100, 餓死=0)。
    pub hunger: u8,
    /// 機嫌 (上機嫌=100, 鬱=0)。
    pub happiness: u8,
    /// 清潔度 (ピカピカ=100, ウンチまみれ=0)。
    pub cleanliness: u8,
    /// HP。0 で死亡。
    pub health: u8,
}

impl Stats {
    /// 孵化直後の初期値。すべて MAX ではないのは「すぐに食事が要る」状況を
    /// 避けつつ「過保護にしすぎても拒否される」境界を体感させるため。
    pub fn starting() -> Self {
        Self {
            hunger: 80,
            happiness: 80,
            cleanliness: 100,
            health: 100,
        }
    }
}

pub struct TamaState {
    pub stage: Stage,
    pub stats: Stats,
    /// 孵化からの経過 tick (Egg / Dead では 0 / 寿命凍結値)。
    pub age_ticks: u64,
    /// プレイヤーがライトを消した状態。decay が緩む代わりにアクション不可。
    pub sleeping: bool,
    /// 何代目のペットか (1 始まり)。
    pub generation: u32,
    /// 歴代最長寿命 (tick)。死亡時に確定。
    pub best_age_ticks: u64,
    /// プレイ開始からの累計 tick (統計用)。
    pub total_ticks: u64,
    /// メッセージログ。直近 6 件を表示する想定。
    pub log: Vec<String>,
    /// 直近アクションを描画 (吹き出し / 表情) に反映するための短命フラグ。
    pub last_action: Option<LastAction>,
    /// `last_action` の残存 tick 数。0 になったら `None` に戻す。
    pub action_flash: u32,
    /// アニメーション用フレームカウンタ (10ticks/sec → 0..u32::MAX で循環)。
    pub anim_frame: u32,
    /// うんちが画面にいくつあるか (清潔度 0 のときに増殖、お風呂で 0)。
    pub poop_count: u8,
    /// ステージ遷移直後の祝福演出の残り tick。0 なら演出なし。
    pub stage_celebration: u32,
    /// 獲得済み称号 (獲得順)。世代をまたいで保持される。
    pub milestones: Vec<Milestone>,
}

impl TamaState {
    pub fn new() -> Self {
        Self {
            stage: Stage::Egg,
            stats: Stats::starting(),
            age_ticks: 0,
            sleeping: false,
            generation: 1,
            best_age_ticks: 0,
            total_ticks: 0,
            log: vec!["たまごが届きました。タップで孵化".into()],
            last_action: None,
            action_flash: 0,
            anim_frame: 0,
            poop_count: 0,
            stage_celebration: 0,
            milestones: Vec::new(),
        }
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    pub fn is_alive(&self) -> bool {
        self.stage != Stage::Dead && self.stage != Stage::Egg
    }

    pub fn is_egg(&self) -> bool {
        self.stage == Stage::Egg
    }

    pub fn is_dead(&self) -> bool {
        self.stage == Stage::Dead
    }

    /// 称号を新規獲得したら true。獲得済みなら何もせず false。
    pub fn unlock_milestone(&mut self, m: Milestone) -> bool {
        if self.milestones.contains(&m) {
            return false;
        }
        self.milestones.push(m);
        true
    }

    /// 獲得済み称号のうち進行順 (`Milestone::ALL`) で最高位のもの。
    pub fn highest_milestone(&self) -> Option<Milestone> {
        Milestone::ALL
            .iter()
            .rev()
            .copied()
            .find(|m| self.milestones.contains(m))
    }
}

impl Default for TamaState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_egg() {
        let s = TamaState::new();
        assert_eq!(s.stage, Stage::Egg);
        assert_eq!(s.generation, 1);
        assert!(s.is_egg());
        assert!(!s.is_alive());
        assert!(!s.is_dead());
    }

    #[test]
    fn stage_save_id_roundtrip() {
        for s in [
            Stage::Egg,
            Stage::Baby,
            Stage::Child,
            Stage::Teen,
            Stage::Adult,
            Stage::Elder,
            Stage::Dead,
        ] {
            assert_eq!(Stage::from_save_id(s.to_save_id()), s);
        }
    }

    #[test]
    fn log_truncation() {
        let mut s = TamaState::new();
        for i in 0..40 {
            s.add_log(format!("msg{}", i));
        }
        assert!(s.log.len() <= 30);
    }

    #[test]
    fn milestone_save_idのroundtrip() {
        for m in Milestone::ALL {
            assert_eq!(Milestone::from_save_id(m.to_save_id()), Some(m));
        }
    }

    #[test]
    fn 不明なmilestone_idはnoneになる() {
        assert_eq!(Milestone::from_save_id(99), None);
    }

    #[test]
    fn unlock_milestoneは重複獲得を防ぐ() {
        let mut s = TamaState::new();
        assert!(s.unlock_milestone(Milestone::Sprout));
        assert!(!s.unlock_milestone(Milestone::Sprout));
        assert_eq!(s.milestones.len(), 1);
    }

    #[test]
    fn highest_milestoneは進行順で最高位を返す() {
        let mut s = TamaState::new();
        assert!(s.highest_milestone().is_none());
        s.unlock_milestone(Milestone::Legend);
        s.unlock_milestone(Milestone::Sprout);
        assert_eq!(s.highest_milestone(), Some(Milestone::Legend));
    }
}
