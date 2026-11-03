use egui::Color32;

use crate::grid::{
    EMPTY, FIRE, MATERIALS, OIL, SAND, SMOKE, STEAM, STONE, WATER, WOOD, material_name,
};

pub const RADIUS_MAX: i32 = 48;
pub const PASSES_MAX: u32 = 16;

const SHORTCUTS: [(char, u8); 9] = [
    ('1', EMPTY),
    ('2', STONE),
    ('3', SAND),
    ('4', WATER),
    ('5', OIL),
    ('6', SMOKE),
    ('7', WOOD),
    ('8', FIRE),
    ('9', STEAM),
];

pub fn material_for_key(key: char) -> Option<u8> {
    SHORTCUTS
        .iter()
        .find(|(shortcut, _)| *shortcut == key)
        .map(|(_, id)| *id)
}

pub struct Controls {
    pub material: u8,
    pub radius: i32,
    pub passes: u32,
    pub paused: bool,
    pub bloom: bool,
    pub clear: bool,
    pub save: bool,
    pub load: bool,
    pub share: bool,
}

impl Controls {
    pub fn new() -> Self {
        Self {
            material: crate::grid::SAND,
            radius: 2,
            passes: 4,
            paused: false,
            bloom: true,
            clear: false,
            save: false,
            load: false,
            share: false,
        }
    }
}

pub struct Swatch {
    pub id: u8,
    pub name: &'static str,
    pub key: Option<char>,
    pub colour: [f32; 4],
}

pub fn palette() -> Vec<Swatch> {
    MATERIALS
        .iter()
        .enumerate()
        .map(|(index, info)| {
            let id = index as u8;
            Swatch {
                id,
                name: material_name(id),
                key: key_for(id),
                colour: info.colour,
            }
        })
        .collect()
}

fn key_for(id: u8) -> Option<char> {
    SHORTCUTS
        .iter()
        .find(|(_, material)| *material == id)
        .map(|(key, _)| *key)
}

pub fn show(ctx: &egui::Context, controls: &mut Controls, fps: f32, notice: &str) {
    egui::Window::new("grains")
        .default_pos([12.0, 12.0])
        .resizable(false)
        .show(ctx, |ui| {
            panel(ui, controls, fps, notice);
        });
}

#[cfg_attr(not(test), allow(dead_code))]
struct LaidOut {
    swatches: Vec<(u8, egui::Rect)>,
    save: egui::Rect,
}

fn panel(ui: &mut egui::Ui, controls: &mut Controls, fps: f32, notice: &str) -> LaidOut {
    if fps > 0.0 {
        ui.label(format!("{fps:.0} fps"));
    } else {
        ui.label("fps");
    }
    let mut rects = Vec::new();
    for row in palette() {
        let rect = swatch(ui, controls, &row);
        rects.push((row.id, rect));
    }
    ui.add(egui::Slider::new(&mut controls.radius, 0..=RADIUS_MAX).text("radius"));
    ui.add(egui::Slider::new(&mut controls.passes, 1..=PASSES_MAX).text("passes"));
    ui.checkbox(&mut controls.paused, "pause");
    ui.checkbox(&mut controls.bloom, "bloom");
    if ui.button("clear").clicked() {
        controls.clear = true;
    }
    let save = ui.button("save");
    if save.clicked() {
        controls.save = true;
    }
    if ui.button("load").clicked() {
        controls.load = true;
    }
    if ui.button("copy link").clicked() {
        controls.share = true;
    }
    if !notice.is_empty() {
        ui.label(notice);
    }
    LaidOut {
        swatches: rects,
        save: save.rect,
    }
}

fn swatch(ui: &mut egui::Ui, controls: &mut Controls, row: &Swatch) -> egui::Rect {
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
        let colour = Color32::from_rgba_unmultiplied(
            byte(row.colour[0]),
            byte(row.colour[1]),
            byte(row.colour[2]),
            byte(row.colour[3]),
        );
        ui.painter().rect_filled(rect, 2.0, colour);
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, Color32::from_gray(80)),
            egui::StrokeKind::Inside,
        );
        let text = match row.key {
            Some(key) => format!("{key}  {}", row.name),
            None => row.name.to_string(),
        };
        let label = ui.selectable_label(controls.material == row.id, text);
        if response.clicked() || label.clicked() {
            controls.material = row.id;
        }
        rect.union(label.rect)
    })
    .inner
}

fn byte(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::{Controls, palette, panel};
    use crate::grid::{MATERIALS, SAND, WOOD};

    #[test]
    fn palette_is_one_row_per_material() {
        let rows = palette();
        assert_eq!(rows.len(), MATERIALS.len());
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row.id, index as u8);
            assert_eq!(row.colour, MATERIALS[index].colour);
            assert_eq!(row.name, crate::grid::material_name(row.id));
        }
        assert_eq!(rows[SAND as usize].key, Some('3'));
        assert_eq!(rows[crate::grid::STONE as usize].key, Some('2'));
        assert_eq!(rows[WOOD as usize].key, Some('7'));
        assert_eq!(rows[crate::grid::FIRE as usize].key, Some('8'));
    }

    #[test]
    fn clicking_a_swatch_selects_that_material() {
        let ctx = egui::Context::default();
        let mut controls = Controls::new();
        assert_eq!(controls.material, SAND);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 640.0));
        let mut wood = None;
        let output = ctx.run_pass(input(screen, &[]), |ctx| {
            ctx.root_ui(|ui| {
                for (id, rect) in panel(ui, &mut controls, 60.0, "").swatches {
                    if id == WOOD {
                        wood = Some(rect);
                    }
                }
            });
        });
        output.drop_without_applying_deltas();
        let wood = wood.expect("wood swatch");
        let pos = wood.center();
        click(&ctx, screen, pos, true, &mut controls);
        click(&ctx, screen, pos, false, &mut controls);
        assert_eq!(controls.material, WOOD);
    }

    #[test]
    fn clicking_save_asks_to_save() {
        let ctx = egui::Context::default();
        let mut controls = Controls::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 640.0));
        let mut save = None;
        let output = ctx.run_pass(input(screen, &[]), |ctx| {
            ctx.root_ui(|ui| {
                save = Some(panel(ui, &mut controls, 60.0, "").save);
            });
        });
        output.drop_without_applying_deltas();
        let pos = save.expect("save button").center();
        click(&ctx, screen, pos, true, &mut controls);
        click(&ctx, screen, pos, false, &mut controls);
        assert!(controls.save);
        assert!(!controls.load);
    }

    fn click(
        ctx: &egui::Context,
        screen: egui::Rect,
        pos: egui::Pos2,
        pressed: bool,
        controls: &mut Controls,
    ) {
        let events = [
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ];
        let output = ctx.run_pass(input(screen, &events), |ctx| {
            ctx.root_ui(|ui| {
                panel(ui, controls, 60.0, "");
            });
        });
        output.drop_without_applying_deltas();
    }

    fn input(screen: egui::Rect, events: &[egui::Event]) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(screen),
            events: events.to_vec(),
            ..Default::default()
        }
    }
}
