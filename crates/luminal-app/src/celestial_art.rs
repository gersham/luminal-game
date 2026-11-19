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
/// A sphere reflects less toward its edge. `z` is the surface normal toward the viewer.
fn limb_shade(color:Color32,z:f32)->Color32 {
    let k=0.62+0.38*z.clamp(0.0,1.0);
    Color32::from_rgb((color.r() as f32*k) as u8,(color.g() as f32*k) as u8,(color.b() as f32*k) as u8)
}
impl CelestialArt {
    pub(super) fn draw(&mut self,ctx:&egui::Context,painter:&egui::Painter,view:&View,index:usize,theme:theme::Theme,p:Pos2,r:f32,now:f64) {
        let body=&view.celestials[index];let star_color=theme.star_color();
        if body.kind==CelestialKind::Star {
            // Corona: two overlapping falloffs, so the light has no rings and no edge.
            fx::glow(painter,p,r*5.0+24.0,star_color,0.42);
            fx::glow(painter,p,r*2.0+10.0,star_color,0.8);
            // Photosphere, hotter at the centre and darker toward the limb.
            let limb=fx::mix(star_color,Color32::BLACK,0.22);
            fx::disc(painter,p,r,fx::mix(star_color,Color32::WHITE,0.62),limb);
            painter.circle_stroke(p,r,Stroke::new(1.0,limb));
            if r>=8.0 {
                // Stable granulation and a slowly moving corona, independent of warp.
                let mut cells=egui::Mesh::default();
                let clear=Color32::from_rgba_premultiplied(0,0,0,0);
                for i in 0..160 {
                    let angle=i as f32*2.399963;let radial=((i as f32+0.5)/160.0).sqrt();
                    let at=p+EVec2::angled(angle)*r*radial*0.93;
                    let pulse=(now as f32*0.4+i as f32).sin();
                    // Dark lanes and bright faculae, soft-edged, in one mesh.
                    let centre=if i%4==0 {fx::light(Color32::WHITE,0.10+0.04*pulse)} else {Color32::from_black_alpha((255.0*(0.085+0.03*pulse)) as u8)};
                    let size=(r*(0.075+0.02*((i*7%5) as f32/4.0))).max(1.2);
                    let base=cells.vertices.len() as u32;
                    cells.colored_vertex(at,centre);
                    for k in 0..8 {cells.colored_vertex(at+EVec2::angled(k as f32*std::f32::consts::TAU/8.0+angle)*size,clear);}
                    for k in 0..8 {cells.add_triangle(base,base+1+k,base+1+(k+1)%8);}
                }
                painter.add(Shape::mesh(cells));
                for i in 0..16 {let a=i as f32*std::f32::consts::TAU/16.0+now as f32*0.025;
                    let d=EVec2::angled(a);let reach=1.22+0.16*(a*7.0).sin()+0.05*(now as f32*0.3+i as f32).sin();
                    fx::plume(painter,p+d*r*0.99,p+d*r*reach,(r*0.05).max(1.2),star_color,0.38);}
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
        if index==1 || body.radius>20000.0 {
            // Air scatters on the sunward limb and thins to nothing around the night side.
            let air=if index==1 {Color32::from_rgb(110,170,255)} else {fx::mix(star_color,Color32::from_rgb(150,190,255),0.5)};
            fx::halo(painter,p,r*0.985,r*1.09+2.0,air,0.55*visible,|d|0.12+0.88*d.dot(light).max(0.0));
        }
        let mut mesh=egui::Mesh::with_texture(atlas.id());
        const SEG:usize=64;const RINGS:usize=20;
        for ring in 0..=RINGS {for step in 0..=SEG {
            let radius=ring as f32/RINGS as f32;let angle=step as f32*std::f32::consts::TAU/SEG as f32;
            let xy=EVec2::angled(angle)*radius;let z=(1.0-radius*radius).max(0.0).sqrt();
            let u=(0.5+xy.x.atan2(z)/std::f32::consts::TAU+0.10*(index as f32*1.71).sin()).clamp(0.005,0.995);
            let v=(0.5+xy.y.asin()/std::f32::consts::PI).clamp(0.005,0.995);
            mesh.vertices.push(egui::epaint::Vertex {pos:p+xy*r,uv:Pos2::new((tile%2) as f32/2.0+u/2.0,(tile/2) as f32/3.0+v/3.0),color:limb_shade(lit_color(xy.dot(light),star_color,visible),z)});
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
