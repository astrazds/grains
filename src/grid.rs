pub const GRID_WIDTH: usize = 512;
pub const GRID_HEIGHT: usize = 288;
pub const CELL_COUNT: usize = GRID_WIDTH * GRID_HEIGHT;

pub(crate) const MATERIAL_SHIFT: u32 = 0;
pub(crate) const VARIATION_SHIFT: u32 = 8;
pub(crate) const LIFE_SHIFT: u32 = 16;
pub(crate) const FLAGS_SHIFT: u32 = 24;

pub const LETTERBOX: [u8; 4] = [20, 20, 23, 255];

pub const EMPTY: u8 = 0;
pub const SAND: u8 = 1;
pub const STONE: u8 = 2;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Cell(u32);

impl Cell {
    pub fn pack(material: u8, variation: u8, life: u8, flags: u8) -> Self {
        Self(
            (u32::from(material) << MATERIAL_SHIFT)
                | (u32::from(variation) << VARIATION_SHIFT)
                | (u32::from(life) << LIFE_SHIFT)
                | (u32::from(flags) << FLAGS_SHIFT),
        )
    }

    pub fn to_bits(self) -> u32 {
        self.0
    }

    pub fn material(self) -> u8 {
        (self.0 >> MATERIAL_SHIFT) as u8
    }

    pub fn variation(self) -> u8 {
        (self.0 >> VARIATION_SHIFT) as u8
    }

    pub fn life(self) -> u8 {
        (self.0 >> LIFE_SHIFT) as u8
    }

    pub fn flags(self) -> u8 {
        (self.0 >> FLAGS_SHIFT) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    pub scale: u32,
    pub origin_x: i32,
    pub origin_y: i32,
}

pub fn cell_at(pixel_x: i32, pixel_y: i32, fit: Fit) -> Option<(u32, u32)> {
    if fit.scale == 0 {
        return None;
    }
    let local_x = pixel_x - fit.origin_x;
    let local_y = pixel_y - fit.origin_y;
    if local_x < 0 || local_y < 0 {
        return None;
    }
    let cell_x = (local_x as u32) / fit.scale;
    let cell_y = (local_y as u32) / fit.scale;
    if cell_x >= GRID_WIDTH as u32 || cell_y >= GRID_HEIGHT as u32 {
        return None;
    }
    Some((cell_x, cell_y))
}

pub fn physical_pixel(css: f64, ratio: f64) -> i32 {
    (css * ratio).round() as i32
}

pub fn letterbox(size: Size) -> Option<Fit> {
    if size.width == 0 || size.height == 0 {
        return None;
    }
    let scale = (size.width / GRID_WIDTH as u32)
        .min(size.height / GRID_HEIGHT as u32)
        .max(1);
    let span_x = GRID_WIDTH as i32 * scale as i32;
    let span_y = GRID_HEIGHT as i32 * scale as i32;
    Some(Fit {
        scale,
        origin_x: (size.width as i32 - span_x) / 2,
        origin_y: (size.height as i32 - span_y) / 2,
    })
}

pub struct Grid {
    cells: Box<[Cell; CELL_COUNT]>,
}

impl Grid {
    pub fn from_fn(mut each: impl FnMut(usize, usize) -> Cell) -> Self {
        let mut cells = vec![Cell::pack(0, 0, 0, 0); CELL_COUNT];
        for y in 0..GRID_HEIGHT {
            for x in 0..GRID_WIDTH {
                cells[index(x, y)] = each(x, y);
            }
        }
        Self {
            cells: heap_cells(cells),
        }
    }

    pub fn demo() -> Self {
        Self::from_fn(|x, y| {
            if y == GRID_HEIGHT - 1 || ledge(x, y) {
                Cell::pack(STONE, 0, 0, 0)
            } else if sand_pile(x, y) {
                Cell::pack(SAND, 0, 0, 0)
            } else {
                Cell::pack(EMPTY, 0, 0, 0)
            }
        })
    }

    pub fn sand_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| cell.material() == SAND)
            .count()
    }

    pub fn cell(&self, x: usize, y: usize) -> Cell {
        self.cells[index(x, y)]
    }

    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        let index = index(x, y);
        self.cells[index] = cell;
    }

    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(self.cells.as_slice())
    }
}

fn index(x: usize, y: usize) -> usize {
    assert!(
        x < GRID_WIDTH && y < GRID_HEIGHT,
        "{x},{y} outside the grid"
    );
    y * GRID_WIDTH + x
}

fn heap_cells(cells: Vec<Cell>) -> Box<[Cell; CELL_COUNT]> {
    cells
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| panic!("grid length is not {CELL_COUNT}"))
}

fn ledge(x: usize, y: usize) -> bool {
    (y == 210 && (40..230).contains(&x))
        || (y == 150 && (270..470).contains(&x))
        || (y == 96 && (100..210).contains(&x))
}

fn sand_pile(x: usize, y: usize) -> bool {
    ((130..180).contains(&x) && (24..64).contains(&y))
        || ((240..280).contains(&x) && (36..96).contains(&y))
}

#[cfg(test)]
mod tests {
    use super::{Cell, Fit, Size, cell_at, letterbox, physical_pixel};

    #[test]
    fn pack_lays_out_the_word() {
        let cell = Cell::pack(0x11, 0x22, 0x33, 0x44);
        assert_eq!(cell.to_bits(), 0x4433_2211);
        assert_eq!(cell.material(), 0x11);
        assert_eq!(cell.variation(), 0x22);
        assert_eq!(cell.life(), 0x33);
        assert_eq!(cell.flags(), 0x44);
    }

    #[test]
    fn letterbox_known_viewports() {
        expect_fit(512, 288, 1, 0, 0);
        expect_fit(1024, 576, 2, 0, 0);
        expect_fit(1024, 600, 2, 0, 12);
        expect_fit(800, 600, 1, 144, 156);
        expect_fit(400, 200, 1, -56, -44);
        expect_fit(515, 288, 1, 1, 0);
        expect_fit(10000, 10, 1, 4744, -139);
        assert!(
            letterbox(Size {
                width: 0,
                height: 288
            })
            .is_none()
        );
        assert!(
            letterbox(Size {
                width: 512,
                height: 0
            })
            .is_none()
        );
    }

    #[test]
    fn cell_at_follows_the_letterbox() {
        let fit = letterbox(Size {
            width: 1024,
            height: 600,
        })
        .unwrap();
        assert_eq!(fit.scale, 2);
        assert_eq!((fit.origin_x, fit.origin_y), (0, 12));
        assert_eq!(cell_at(0, 12, fit), Some((0, 0)));
        assert_eq!(cell_at(1, 13, fit), Some((0, 0)));
        assert_eq!(cell_at(2, 12, fit), Some((1, 0)));
        assert_eq!(cell_at(1023, 587, fit), Some((511, 287)));
        assert_eq!(cell_at(0, 11, fit), None);
        assert_eq!(cell_at(1023, 588, fit), None);
        assert_eq!(cell_at(-1, 12, fit), None);

        let wide = letterbox(Size {
            width: 1100,
            height: 600,
        })
        .unwrap();
        assert_eq!((wide.origin_x, wide.scale), (38, 2));
        assert_eq!(cell_at(37, 12, wide), None);
        assert_eq!(cell_at(38, 11, wide), None);
        assert_eq!(cell_at(38, 12, wide), Some((0, 0)));
        assert_eq!(cell_at(38 + 1023, 12 + 575, wide), Some((511, 287)));
        assert_eq!(cell_at(38 + 1024, 12, wide), None);

        let crop = letterbox(Size {
            width: 400,
            height: 200,
        })
        .unwrap();
        assert_eq!((crop.origin_x, crop.origin_y, crop.scale), (-56, -44, 1));
        assert_eq!(cell_at(0, 0, crop), Some((56, 44)));
        assert_eq!(cell_at(399, 199, crop), Some((455, 243)));
    }

    #[test]
    fn physical_pixel_scales_css_before_the_grid() {
        let fit = Fit {
            scale: 2,
            origin_x: 0,
            origin_y: 12,
        };
        let x = physical_pixel(1.0, 2.0);
        let y = physical_pixel(6.0, 2.0);
        assert_eq!((x, y), (2, 12));
        assert_eq!(cell_at(x, y, fit), Some((1, 0)));
        assert_eq!(
            cell_at(physical_pixel(2.0, 1.0), physical_pixel(12.0, 1.0), fit),
            Some((1, 0))
        );
        assert_eq!(
            cell_at(physical_pixel(0.8, 2.5), physical_pixel(4.8, 2.5), fit),
            Some((1, 0))
        );
        assert_eq!(
            cell_at(physical_pixel(0.0, 2.0), physical_pixel(5.4, 2.0), fit),
            None
        );
    }

    #[test]
    fn from_fn_stores_the_placed_cell() {
        let grid = super::Grid::from_fn(|x, y| {
            if x == 511 && y == 287 {
                Cell::pack(0x11, 0x22, 0x33, 0x44)
            } else {
                Cell::pack(0, 0, 0, 0)
            }
        });
        assert_eq!(grid.cell(511, 287).to_bits(), 0x4433_2211);
        assert_eq!(grid.cell(1, 0).to_bits(), 0);
    }

    fn expect_fit(width: u32, height: u32, scale: u32, origin_x: i32, origin_y: i32) {
        let fit = letterbox(Size { width, height }).unwrap();
        assert_eq!(fit.scale, scale);
        assert_eq!(fit.origin_x, origin_x);
        assert_eq!(fit.origin_y, origin_y);
    }
}
