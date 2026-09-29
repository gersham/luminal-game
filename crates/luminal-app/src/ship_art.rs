//! Curds-generated recognition sheets embedded in the executable.
use super::*;
use luminal_core::world::ShipClass;
const SHEETS:[&[u8];4]=[
    include_bytes!("../../../assets/ships/luminal.png"),
    include_bytes!("../../../assets/ships/grim-dark.png"),
    include_bytes!("../../../assets/ships/imperium.png"),
    include_bytes!("../../../assets/ships/culture.png"),
];
const BANDS:[[(u32,u32);5];4]=[
    [(10,145),(145,337),(337,522),(522,728),(728,1005)],
    [(10,171),(173,318),(320,470),(607,791),(793,1017)],
    [(10,155),(158,303),(306,489),(490,688),(689,1015)],
    [(40,175),(195,340),(365,505),(520,740),(755,985)],
];
#[derive(Default)]
pub(super) struct ShipArt {textures:[Option<egui::TextureHandle>;4]}
fn decode(bytes:&[u8])->egui::ColorImage {
    let image=image::load_from_memory(bytes).expect("embedded ship sheet").to_rgb8();
    let size=[image.width() as usize,image.height() as usize];
    let pixels=image.pixels().map(|p| {
        // Treat generated white lines as coverage, not a black rectangular card.
        let alpha=*p.0.iter().max().unwrap();
        egui::Color32::from_white_alpha(if alpha<12 {0} else {alpha})
    }).collect();
    egui::ColorImage::new(size,pixels)
}
impl ShipArt {
    pub(super) fn draw(&mut self,ctx:&egui::Context,painter:&egui::Painter,rect:Rect,theme:theme::Theme,class:ShipClass,color:Color32) {
        let index=theme as usize;
        let texture=self.textures[index].get_or_insert_with(||ctx.load_texture(format!("ship-recognition-{index}"),decode(SHEETS[index]),egui::TextureOptions::LINEAR));
        let tier=ShipClass::COMBAT.iter().position(|c|*c==class).unwrap_or(0);
        let (top,bottom)=BANDS[index][tier];
        let source_size=EVec2::new(1024.0,(bottom-top) as f32);
        let scale=(rect.width()/source_size.x).min(rect.height()/source_size.y);
        let target=Rect::from_center_size(rect.center(),source_size*scale);
        let uv=Rect::from_min_max(Pos2::new(0.0,top as f32/1024.0),Pos2::new(1.0,bottom as f32/1024.0));
        painter.image(texture.id(),target,uv,color);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_embedded_sheet_decodes_and_every_class_crop_contains_lines() {
        for (index,bytes) in SHEETS.iter().enumerate() {
            let image=decode(bytes);assert_eq!(image.size,[1024,1024]);
            for (start,end) in BANDS[index] {
                let pixels=&image.pixels[start as usize*1024..end as usize*1024];
                assert!(pixels.iter().filter(|p|p.a()>80).count()>100);
                assert!(pixels.iter().filter(|p|p.a()==0).count()>pixels.len()/2);
            }
        }
    }
}
