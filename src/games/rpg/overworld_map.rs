//! Overworld (village) — fixed hand-crafted map.
//!
//! Returns a `DungeonMap` compatible with the dungeon explore renderer
//! and movement code. Differences vs. dungeon maps:
//!
//! - `is_overworld == true`
//! - `floor_num == 0` (used by `floor_theme` to pick `Village` colors)
//! - `monsters` is always empty; satiety/turn ticks are skipped in logic
//! - All cells start `revealed = true` and `visited = true` so the village
//!   has no fog of war (you can see the whole layout immediately)
//! - Tiles do NOT get marked `event_done = true` after interaction, so the
//!   player can repeatedly visit the shop / NPC / inn

use super::state::{CellType, DungeonMap, Facing, MapCell, Tile};

/// Hand-laid 27×13 village layout. Each char is one tile.
///
/// Legend:
/// - `#` = wall
/// - `.` = floor
/// - `@` = player spawn (also a floor)
/// - `R` = Reception NPC
/// - `B` = Blacksmith NPC
/// - `V` = Villager NPC
/// - `S` = Shop tile
/// - `Q` = Quest board tile
/// - `I` = Inn tile
/// - `T` = Shrine tile
/// - `D` = Dungeon entrance
const VILLAGE: &[&str] = &[
    "###########################",
    "#.........................#",
    "#..R................B.....#",
    "#.........................#",
    "#............V............#",
    "#......V.................T#",
    "#.........................#",
    "#..S................I.....#",
    "#.........................#",
    "#......Q.............D....#",
    "#.........................#",
    "#............@............#",
    "###########################",
];

/// Build the village map. Same struct layout as a dungeon map so all the
/// existing rendering / movement code Just Works.
pub fn generate_overworld() -> DungeonMap {
    let height = VILLAGE.len();
    let width = VILLAGE[0].chars().count();

    let mut grid: Vec<Vec<MapCell>> = (0..height)
        .map(|_| {
            (0..width)
                .map(|_| MapCell {
                    tile: Tile::Wall,
                    cell_type: CellType::Corridor,
                    visited: true,
                    revealed: true,
                    event_done: false,
                    room_id: None,
                })
                .collect()
        })
        .collect();

    let mut player_x = 1;
    let mut player_y = 1;

    for (y, row) in VILLAGE.iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            let cell = &mut grid[y][x];
            match ch {
                '#' => {
                    cell.tile = Tile::Wall;
                }
                '.' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                }
                '@' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    player_x = x;
                    player_y = y;
                }
                'R' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::ReceptionNpc;
                }
                'B' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::BlacksmithNpc;
                }
                'V' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::VillagerNpc;
                }
                'S' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::ShopTile;
                }
                'Q' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::QuestBoardTile;
                }
                'I' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::InnTile;
                }
                'T' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::ShrineTile;
                }
                'D' => {
                    cell.tile = Tile::RoomFloor;
                    cell.room_id = Some(0);
                    cell.cell_type = CellType::DungeonEntrance;
                }
                _ => {}
            }
        }
    }

    // Single "room" covering the whole walkable area so the visibility
    // routine treats the village as one open courtyard.
    let rooms = vec![super::state::Room {
        x: 1,
        y: 1,
        w: width.saturating_sub(2),
        h: height.saturating_sub(2),
    }];

    DungeonMap {
        floor_num: 0,
        width,
        height,
        grid,
        player_x,
        player_y,
        last_dir: Facing::North,
        rooms,
        monsters: Vec::new(),
        is_overworld: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overworld_loads() {
        let map = generate_overworld();
        assert!(map.is_overworld);
        assert_eq!(map.floor_num, 0);
        assert!(map.monsters.is_empty());
        assert_eq!(map.width, 27);
        assert_eq!(map.height, 13);
    }

    #[test]
    fn overworld_player_on_walkable_tile() {
        let map = generate_overworld();
        assert!(map.player_cell().is_walkable());
    }

    #[test]
    fn overworld_contains_all_facility_tiles() {
        let map = generate_overworld();
        let types: Vec<CellType> = map
            .grid
            .iter()
            .flatten()
            .map(|c| c.cell_type)
            .collect();
        for required in &[
            CellType::DungeonEntrance,
            CellType::ShopTile,
            CellType::QuestBoardTile,
            CellType::InnTile,
            CellType::ShrineTile,
            CellType::ReceptionNpc,
            CellType::BlacksmithNpc,
            CellType::VillagerNpc,
        ] {
            assert!(
                types.contains(required),
                "village missing tile {:?}",
                required
            );
        }
    }

    #[test]
    fn overworld_is_fully_revealed() {
        let map = generate_overworld();
        for row in &map.grid {
            for cell in row {
                assert!(cell.revealed);
                assert!(cell.visited);
            }
        }
    }
}
