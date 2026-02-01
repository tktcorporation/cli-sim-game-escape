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
