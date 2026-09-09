use crate::map_marker::tests::native::Fixture;
use crate::offsets::{body_loc, d2common, inventory, inventory_grid, items_txt, stat_list, unit};

pub const PLAYER: usize = 0x100;
pub const MERC: usize = 0x104;

pub fn fixture(values: &[(u32, i32)]) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.ctx.d2_common = fixture.ctx.d2_client;
    fixture.seed(PLAYER, fixture.address(0x2000));
    fixture.seed(MERC, fixture.address(0x2000));
    fixture.seed(0x2000 + unit::CLASS, 271);
    fixture.seed(0x2000 + unit::UNIT_TYPE, 1);
    fixture.seed(
        0x2000 + stat_list::UNIT_TO_STATS_LIST,
        fixture.address(0x8000),
    );
    fixture.seed(0x8000 + stat_list::SL_FLAGS, 0);
    fixture.seed(0x8000 + stat_list::SL_PSTAT, fixture.address(0x9000));
    fixture.seed(
        0x8000 + stat_list::SL_STAT_COUNT,
        u32::try_from(values.len()).unwrap(),
    );
    for (index, &(id, value)) in values.iter().enumerate() {
        fixture.seed(0x9000 + index * 8, id << 16);
        fixture.seed(0x9004 + index * 8, u32::from_ne_bytes(value.to_ne_bytes()));
    }
    fixture.seed(0x2000 + unit::INVENTORY, fixture.address(0xa000));
    fixture.seed(0xa000 + inventory::GRIDS, fixture.address(0xb000));
    fixture.seed(0xb000 + inventory_grid::PP_ITEMS, fixture.address(0xc000));
    fixture.seed(0xc000 + body_loc::RARM * 4, fixture.address(0xd000));
    fixture.seed(0xd000 + unit::CLASS, 1);
    fixture.seed(d2common::ITEMS_TXT, fixture.address(0xe000));
    fixture.seed(0xe000 + items_txt::RECORD_SIZE + items_txt::STR_BONUS, 100);
    fixture.seed(0xe000 + items_txt::RECORD_SIZE + items_txt::DEX_BONUS, 50);
    fixture
}
