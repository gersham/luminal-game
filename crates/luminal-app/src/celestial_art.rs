//! Surface atlases shaded in the renderer; no baked sunlight or simulation changes.
use super::*;
const SHEETS:[&[u8];4]=[
    include_bytes!("../../../assets/celestials/luminal.png"),
    include_bytes!("../../../assets/celestials/grim-dark.png"),
    include_bytes!("../../../assets/celestials/imperium.png"),
    include_bytes!("../../../assets/celestials/culture.png"),
];
#[derive(Default)]
pub(super) struct CelestialArt {textures:[Option<egui::TextureHandle>;4]}
impl theme::Theme {
    pub fn star_color(self)->Color32 {match self {
        Self::Luminal=>Color32::from_rgb(255,235,190),
        Self::GrimDark=>Color32::from_rgb(255,155,95),
        Self::Imperium=>Color32::from_rgb(255,212,150),
        Self::Culture=>Color32::from_rgb(205,225,255),
    }}
}
fn lit_color(light:f32,tint:Color32,visibility:f32)->Color32 {
    let strength=0.065+0.935*light.max(0.0)*visibility;
    let channel=|c:u8|(255.0*strength*(0.65+0.35*c as f32/255.0)).clamp(0.0,255.0) as u8;
    Color32::from_rgb(channel(tint.r()),channel(tint.g()),channel(tint.b()))
}
impl CelestialArt {
    pub(super) fn draw(&mut self,ctx:&egui::Context,painter:&egui::Painter,view:&View,index:usize,theme:theme::Theme,p:Pos2,r:f32,now:f64) {
        let body=&view.celestials[index];let star_color=theme.star_color();
        if body.kind==CelestialKind::Star {
            for layer in (1..=10).rev() {painter.circle_filled(p,r*(1.0+layer as f32*0.10),star_color.gamma_multiply(0.018));}
            painter.circle_filled(p,r,star_color);
            if r>=8.0 {
                // Stable granulation and a slowly moving corona, independent of warp.
                for i in 0..120 {
                    let angle=i as f32*2.399963;let radial=((i as f32+0.5)/120.0).sqrt();
                    let at=p+EVec2::angled(angle)*r*radial*0.96;
                    let flicker=0.045+0.02*(now as f32*0.4+i as f32).sin();
                    painter.circle_filled(at,(r*0.055).max(0.7),Color32::BLACK.gamma_multiply(flicker));
                }
                painter.circle_filled(p,r*0.55,Color32::WHITE.gamma_multiply(0.10));
                for i in 0..12 {let a=i as f32*std::f32::consts::TAU/12.0+now as f32*0.025;
                    let d=EVec2::angled(a);painter.line_segment([p+d*r*1.01,p+d*r*(1.09+0.025*(a*7.0).sin())],Stroke::new((r*0.018).max(0.7),star_color.gamma_multiply(0.17)));}
            }
            return;
        }
        if r<5.0 {painter.circle_filled(p,r,celestial_color(body.kind));return;}
        let atlas=self.textures[theme as usize].get_or_insert_with(|| {
            let img=image::load_from_memory(SHEETS[theme as usize]).expect("celestial atlas").to_rgba8();
            ctx.load_texture(format!("celestials-{}",theme as usize),egui::ColorImage::from_rgba_unmultiplied([img.width() as usize,img.height() as usize],img.as_raw()),egui::TextureOptions::LINEAR)
        });
        let tile=if index==1 {0} else if body.radius>20000.0 {1} else if body.kind==CelestialKind::Moon {match index%3 {0=>3,1=>4,_=>5}} else if (body.pos-view.celestials[0].pos).length()>2.0*AU {3} else {2};
        let direction=(view.celestials[0].pos-body.pos).normalized();
        let light=EVec2::new(direction.x as f32,-direction.y as f32);
        let visible=view.system.stellar_visibility(body.pos,view.time,Some(index)) as f32;
        if index==1 || body.radius>20000.0 {painter.circle_stroke(p,r+1.0,Stroke::new(2.0,star_color.gamma_multiply(0.15*visible)));}
        let mut mesh=egui::Mesh::with_texture(atlas.id());
        const SEG:usize=64;const RINGS:usize=20;
        for ring in 0..=RINGS {for step in 0..=SEG {
            let radius=ring as f32/RINGS as f32;let angle=step as f32*std::f32::consts::TAU/SEG as f32;
            let xy=EVec2::angled(angle)*radius;let z=(1.0-radius*radius).max(0.0).sqrt();
            let u=(0.5+xy.x.atan2(z)/std::f32::consts::TAU+0.10*(index as f32*1.71).sin()).clamp(0.005,0.995);
            let v=(0.5+xy.y.asin()/std::f32::consts::PI).clamp(0.005,0.995);
            mesh.vertices.push(egui::epaint::Vertex {pos:p+xy*r,uv:Pos2::new((tile%2) as f32/2.0+u/2.0,(tile/2) as f32/3.0+v/3.0),color:lit_color(xy.dot(light),star_color,visible)});
        }}
        for ring in 0..RINGS {for step in 0..SEG {let a=(ring*(SEG+1)+step) as u32;let b=a+(SEG+1) as u32;
            mesh.indices.extend_from_slice(&[a,b,a+1,a+1,b,b+1]);}}
        painter.add(Shape::mesh(mesh));
        painter.circle_stroke(p,r,Stroke::new(0.7,star_color.gamma_multiply(0.12)));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlases_decode_and_star_lighting_has_a_dark_hemisphere() {
        for bytes in SHEETS {let image=image::load_from_memory(bytes).unwrap();assert_eq!((image.width(),image.height()),(1024,1536));}
        for theme in theme::Theme::ALL {
            let day=lit_color(1.0,theme.star_color(),1.0);let night=lit_color(-1.0,theme.star_color(),1.0);
            assert!(day.r()>night.r()*5 && day.b()>night.b()*5);
            assert_eq!(lit_color(1.0,theme.star_color(),0.0),night);
        }
        assert_ne!(theme::Theme::Culture.star_color(),theme::Theme::GrimDark.star_color());
    }
}
