/// Game state and logic for the escape room game.

#[derive(Clone, Debug, PartialEq)]
pub enum Room {
    /// Starting room - a dimly lit office
    Office,
    /// A storage closet connected to the office
    Storage,
    /// A hallway leading to the exit
    Hallway,
    /// The locked exit door
    Exit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Key,
    Flashlight,
    Note,
    Screwdriver,
    Keycard,
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::Key => "古い鍵",
            Item::Flashlight => "懐中電灯",
            Item::Note => "メモ用紙",
            Item::Screwdriver => "ドライバー",
            Item::Keycard => "キーカード",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GamePhase {
    Playing,
    Escaped,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputMode {
    /// Normal exploration mode - arrow keys / number selection
    Explore,
    /// Viewing inventory
    Inventory,
}

/// An action the player can take in the current context.
#[derive(Clone, Debug)]
pub struct Action {
    pub label: String,
    pub key: char,
}

/// Represents a message log entry.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub text: String,
    pub is_important: bool,
}

pub struct GameState {
    pub room: Room,
    pub phase: GamePhase,
    pub input_mode: InputMode,
    pub inventory: Vec<Item>,
    pub log: Vec<LogEntry>,
    pub actions: Vec<Action>,
    // Room-specific state
    pub office_desk_searched: bool,
    pub office_drawer_opened: bool,
    pub storage_shelf_searched: bool,
    pub storage_vent_opened: bool,
    pub hallway_panel_opened: bool,
    pub exit_unlocked: bool,
}

impl GameState {
    pub fn new() -> Self {
        let mut state = GameState {
            room: Room::Office,
            phase: GamePhase::Playing,
            input_mode: InputMode::Explore,
            inventory: Vec::new(),
            log: Vec::new(),
            actions: Vec::new(),
            office_desk_searched: false,
            office_drawer_opened: false,
            storage_shelf_searched: false,
            storage_vent_opened: false,
            hallway_panel_opened: false,
            exit_unlocked: false,
        };
        state.add_log("目が覚めると、薄暗いオフィスの中にいた...", true);
        state.add_log("ここから脱出しなければ。", false);
        state.update_actions();
        state
    }

    pub fn add_log(&mut self, text: &str, is_important: bool) {
        self.log.push(LogEntry {
            text: text.to_string(),
            is_important,
        });
        // Keep log manageable
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    pub fn has_item(&self, item: &Item) -> bool {
        self.inventory.contains(item)
    }

    pub fn room_description(&self) -> &str {
        match self.room {
            Room::Office => "【オフィス】\n薄暗いオフィス。デスクと本棚がある。\n北側にドアが見える。",
            Room::Storage => "【倉庫】\n雑然とした倉庫。棚にいろいろな物が置かれている。\n壁に換気口がある。南側にオフィスへの扉がある。",
            Room::Hallway => "【廊下】\n長い廊下。東の壁にパネルがある。\n北に出口のドア、南にオフィスへの扉がある。",
            Room::Exit => {
                if self.exit_unlocked {
                    "【出口】\nドアは開いている！外に出られる！"
                } else {
                    "【出口】\n重厚なドアにカードリーダーが付いている。\nキーカードが必要なようだ。"
                }
            }
        }
    }

    pub fn update_actions(&mut self) {
        self.actions.clear();
        match self.room {
            Room::Office => {
                if !self.office_desk_searched {
                    self.actions.push(Action {
                        label: "デスクを調べる".into(),
                        key: '1',
                    });
                }
                if !self.office_drawer_opened {
                    if self.has_item(&Item::Key) {
                        self.actions.push(Action {
                            label: "鍵で引き出しを開ける".into(),
                            key: '2',
                        });
                    } else {
                        self.actions.push(Action {
                            label: "引き出し (鍵がかかっている)".into(),
                            key: '2',
                        });
                    }
                }
                self.actions.push(Action {
                    label: "北のドア → 廊下へ".into(),
                    key: 'n',
                });
            }
            Room::Storage => {
                if !self.storage_shelf_searched {
                    self.actions.push(Action {
                        label: "棚を調べる".into(),
                        key: '1',
                    });
                }
                if !self.storage_vent_opened {
                    if self.has_item(&Item::Screwdriver) {
                        self.actions.push(Action {
                            label: "ドライバーで換気口を開ける".into(),
                            key: '2',
                        });
                    } else {
                        self.actions.push(Action {
                            label: "換気口 (ネジで固定されている)".into(),
                            key: '2',
                        });
                    }
                }
                self.actions.push(Action {
                    label: "南のドア → オフィスへ".into(),
                    key: 's',
                });
            }
            Room::Hallway => {
                if !self.hallway_panel_opened {
                    if self.has_item(&Item::Screwdriver) {
                        self.actions.push(Action {
                            label: "ドライバーでパネルを開ける".into(),
                            key: '1',
                        });
                    } else {
                        self.actions.push(Action {
                            label: "壁のパネル (ネジで固定されている)".into(),
                            key: '1',
                        });
                    }
                }
                self.actions.push(Action {
                    label: "北 → 出口へ".into(),
                    key: 'n',
                });
                self.actions.push(Action {
                    label: "南 → オフィスへ".into(),
                    key: 's',
                });
                self.actions.push(Action {
                    label: "西 → 倉庫へ".into(),
                    key: 'w',
                });
            }
            Room::Exit => {
                if !self.exit_unlocked && self.has_item(&Item::Keycard) {
                    self.actions.push(Action {
                        label: "キーカードを使う".into(),
                        key: '1',
                    });
                }
                if self.exit_unlocked {
                    self.actions.push(Action {
                        label: "外に出る！".into(),
                        key: '1',
                    });
                }
                self.actions.push(Action {
                    label: "南 → 廊下へ".into(),
                    key: 's',
                });
            }
        }
    }

    pub fn handle_action(&mut self, key: char) {
        if self.phase != GamePhase::Playing {
            return;
        }

        // Find matching action
        let action_exists = self.actions.iter().any(|a| a.key == key);
        if !action_exists {
            return;
        }

        match self.room {
            Room::Office => self.handle_office_action(key),
            Room::Storage => self.handle_storage_action(key),
            Room::Hallway => self.handle_hallway_action(key),
            Room::Exit => self.handle_exit_action(key),
        }

        self.update_actions();
    }

    fn handle_office_action(&mut self, key: char) {
        match key {
            '1' if !self.office_desk_searched => {
                self.office_desk_searched = true;
                self.add_log("デスクの上を調べた。", false);
                self.add_log("メモ用紙を見つけた！「暗証番号: 4821」と書かれている。", true);
                self.inventory.push(Item::Note);
            }
            '2' => {
                if self.has_item(&Item::Key) && !self.office_drawer_opened {
                    self.office_drawer_opened = true;
                    self.add_log("鍵で引き出しを開けた！", false);
                    self.add_log("懐中電灯を見つけた！", true);
                    self.inventory.push(Item::Flashlight);
                } else if !self.office_drawer_opened {
                    self.add_log("引き出しには鍵がかかっている。鍵が必要だ。", false);
                }
            }
            'n' => {
                self.room = Room::Hallway;
                self.add_log("廊下に出た。", false);
            }
            _ => {}
        }
    }

    fn handle_storage_action(&mut self, key: char) {
        match key {
            '1' if !self.storage_shelf_searched => {
                self.storage_shelf_searched = true;
                self.add_log("棚を調べた。", false);
                self.add_log("古い鍵を見つけた！", true);
                self.inventory.push(Item::Key);
            }
            '2' => {
                if self.has_item(&Item::Screwdriver) && !self.storage_vent_opened {
                    self.storage_vent_opened = true;
                    self.add_log("ドライバーで換気口のネジを外した！", false);
                    self.add_log("中にキーカードが隠されていた！", true);
                    self.inventory.push(Item::Keycard);
                } else if !self.storage_vent_opened {
                    self.add_log("換気口はネジで固定されている。外す道具が必要だ。", false);
                }
            }
            's' => {
                self.room = Room::Office;
                self.add_log("オフィスに戻った。", false);
            }
            _ => {}
        }
    }

    fn handle_hallway_action(&mut self, key: char) {
        match key {
            '1' => {
                if self.has_item(&Item::Screwdriver) && !self.hallway_panel_opened {
                    self.hallway_panel_opened = true;
                    self.add_log("パネルをドライバーで開けた！", false);
                    self.add_log("中に非常用の配線図があった。特に役に立たなそうだ。", false);
                } else if !self.hallway_panel_opened {
                    self.add_log("パネルはネジで固定されている。外す道具が必要だ。", false);
                }
            }
            'n' => {
                self.room = Room::Exit;
                self.add_log("出口の前に来た。", false);
            }
            's' => {
                self.room = Room::Office;
                self.add_log("オフィスに戻った。", false);
            }
            'w' => {
                if self.has_item(&Item::Flashlight) {
                    self.room = Room::Storage;
                    self.add_log("懐中電灯で暗い倉庫を照らしながら入った。", false);
                } else {
                    self.add_log("倉庫は真っ暗だ。明かりがないと危険だ。", false);
                }
            }
            _ => {}
        }
    }

    fn handle_exit_action(&mut self, key: char) {
        match key {
            '1' => {
                if self.exit_unlocked {
                    self.phase = GamePhase::Escaped;
                    self.add_log("ドアを開けて外に出た！", true);
                    self.add_log("", false);
                    self.add_log("★★★ 脱出成功！おめでとう！ ★★★", true);
                } else if self.has_item(&Item::Keycard) {
                    self.exit_unlocked = true;
                    self.add_log("キーカードをリーダーにかざした...", false);
                    self.add_log("ピッ！ドアのロックが解除された！", true);
                }
            }
            's' => {
                self.room = Room::Hallway;
                self.add_log("廊下に戻った。", false);
            }
            _ => {}
        }
    }

    pub fn inventory_display(&self) -> Vec<String> {
        if self.inventory.is_empty() {
            vec!["(何も持っていない)".to_string()]
        } else {
            self.inventory.iter().map(|i| i.name().to_string()).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Game action tests (same actions triggered by keyboard or tap) ──

    #[test]
    fn initial_state() {
        let gs = GameState::new();
        assert_eq!(gs.room, Room::Office);
        assert_eq!(gs.phase, GamePhase::Playing);
        assert_eq!(gs.input_mode, InputMode::Explore);
        assert!(gs.inventory.is_empty());
        assert!(!gs.actions.is_empty());
    }

    #[test]
    fn office_actions_available() {
        let gs = GameState::new();
        let keys: Vec<char> = gs.actions.iter().map(|a| a.key).collect();
        assert!(keys.contains(&'1')); // デスクを調べる
        assert!(keys.contains(&'2')); // 引き出し
        assert!(keys.contains(&'n')); // 北のドア
    }

    #[test]
    fn search_desk_gives_note() {
        let mut gs = GameState::new();
        gs.handle_action('1');
        assert!(gs.office_desk_searched);
        assert!(gs.has_item(&Item::Note));
    }

    #[test]
    fn locked_drawer_without_key() {
        let mut gs = GameState::new();
        gs.handle_action('2');
        // Drawer stays locked, no item gained
        assert!(!gs.office_drawer_opened);
        assert!(!gs.has_item(&Item::Flashlight));
    }

    #[test]
    fn move_to_hallway() {
        let mut gs = GameState::new();
        gs.handle_action('n');
        assert_eq!(gs.room, Room::Hallway);
    }

    #[test]
    fn hallway_to_storage_requires_flashlight() {
        let mut gs = GameState::new();
        gs.handle_action('n'); // Office → Hallway
        gs.handle_action('w'); // Try Hallway → Storage (need flashlight)
        assert_eq!(gs.room, Room::Hallway); // Still in hallway

        gs.inventory.push(Item::Flashlight);
        gs.update_actions();
        gs.handle_action('w'); // Now should work
        assert_eq!(gs.room, Room::Storage);
    }

    #[test]
    fn storage_shelf_gives_key() {
        let mut gs = GameState::new();
        gs.room = Room::Storage;
        gs.update_actions();
        gs.handle_action('1');
        assert!(gs.storage_shelf_searched);
        assert!(gs.has_item(&Item::Key));
    }

    #[test]
    fn full_game_walkthrough() {
        let mut gs = GameState::new();

        // 1. Search desk → get Note
        gs.handle_action('1');
        assert!(gs.has_item(&Item::Note));

        // 2. Go to hallway
        gs.handle_action('n');
        assert_eq!(gs.room, Room::Hallway);

        // 3. Can't enter storage without flashlight
        gs.handle_action('w');
        assert_eq!(gs.room, Room::Hallway);

        // 4. Go back to office, need key first
        gs.handle_action('s');
        assert_eq!(gs.room, Room::Office);

        // Shortcut: give ourselves the items for full walkthrough
        gs.inventory.push(Item::Key);
        gs.update_actions();
        gs.handle_action('2'); // Open drawer with key → Flashlight
        assert!(gs.has_item(&Item::Flashlight));

        gs.inventory.push(Item::Screwdriver);

        // Go to hallway then storage
        gs.handle_action('n');
        gs.handle_action('w');
        assert_eq!(gs.room, Room::Storage);

        // Search shelf (already have key from shortcut)
        // Open vent with screwdriver → Keycard
        gs.handle_action('2');
        assert!(gs.has_item(&Item::Keycard));

        // Go back to exit
        gs.handle_action('s'); // Storage → Office
        gs.handle_action('n'); // Office → Hallway
        gs.handle_action('n'); // Hallway → Exit
        assert_eq!(gs.room, Room::Exit);

        // Use keycard
        gs.handle_action('1');
        assert!(gs.exit_unlocked);

        // Walk out
        gs.handle_action('1');
        assert_eq!(gs.phase, GamePhase::Escaped);
    }

    #[test]
    fn input_mode_toggle() {
        let mut gs = GameState::new();
        assert_eq!(gs.input_mode, InputMode::Explore);

        gs.input_mode = InputMode::Inventory;
        assert_eq!(gs.input_mode, InputMode::Inventory);

        gs.input_mode = InputMode::Explore;
        assert_eq!(gs.input_mode, InputMode::Explore);
    }

    #[test]
    fn handle_action_ignored_when_escaped() {
        let mut gs = GameState::new();
        gs.phase = GamePhase::Escaped;
        let log_len = gs.log.len();
        gs.handle_action('1');
        // No new log entries since action is ignored
        assert_eq!(gs.log.len(), log_len);
    }

    #[test]
    fn handle_action_invalid_key_ignored() {
        let mut gs = GameState::new();
        let log_len = gs.log.len();
        gs.handle_action('z'); // Not a valid action
        assert_eq!(gs.log.len(), log_len);
    }

    #[test]
    fn inventory_display_empty() {
        let gs = GameState::new();
        let display = gs.inventory_display();
        assert_eq!(display, vec!["(何も持っていない)"]);
    }

    #[test]
    fn inventory_display_with_items() {
        let mut gs = GameState::new();
        gs.inventory.push(Item::Key);
        gs.inventory.push(Item::Flashlight);
        let display = gs.inventory_display();
        assert_eq!(display, vec!["古い鍵", "懐中電灯"]);
    }

    #[test]
    fn actions_update_after_room_change() {
        let mut gs = GameState::new();
        let office_keys: Vec<char> = gs.actions.iter().map(|a| a.key).collect();
        assert!(office_keys.contains(&'n'));

        gs.handle_action('n'); // Move to hallway
        let hallway_keys: Vec<char> = gs.actions.iter().map(|a| a.key).collect();
        assert!(hallway_keys.contains(&'s')); // Back to office
        assert!(hallway_keys.contains(&'w')); // To storage
        assert!(hallway_keys.contains(&'n')); // To exit
    }

    #[test]
    fn log_truncation() {
        let mut gs = GameState::new();
        for i in 0..60 {
            gs.add_log(&format!("message {}", i), false);
        }
        assert!(gs.log.len() <= 50);
    }
}
