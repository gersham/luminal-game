//! Authored, setting-inspired systems. These are original locations, not canonical atlases.
//! Indices 0/1/2 are the star, escort homeworld and station moon for scenario setup.
use super::theme::Theme;
use luminal_core::{celestial::{System,Celestial,CelestialKind,Orbit},kinematics::Vec2,rng::Rng,units::AU};
impl Theme {
    pub fn star_name(self)->&'static str {match self {Self::Luminal=>"Sol",Self::GrimDark=>"Vesper",Self::Imperium=>"Kestrel",Self::Culture=>"Quiet Reach"}}
    pub fn system_description(self)->&'static str {match self {
        Self::Luminal=>"Eight familiar planets and thirteen moons",
        Self::GrimDark=>"Forge worlds, shrine moons and distant gas giants",
        Self::Imperium=>"A compact frontier trade system with two gas giants",
        Self::Culture=>"A spacious inhabited system with rich moon groups",
    }}
    pub fn star_system(self,seed:u64)->System {
        if self==Self::Luminal {return luminal_core::sol::system(seed);}
        let (mass,radius)=match self {Self::GrimDark=>(0.82,580000.0),Self::Imperium=>(0.94,650000.0),_=>(1.12,755000.0)};
        let mut system=System {bodies:vec![Celestial {name:self.star_name().into(),kind:CelestialKind::Star,gm:1.3271244e11*mass,radius,orbit:Orbit::Fixed(Vec2::ZERO)}]};
        // (name, parent, orbital distance km, physical radius km, GM km³/s²).
        // Parents always precede children; moon distances are intentionally varied.
        let bodies:&[(&str,usize,f64,f64,f64)]=match self {
            Self::GrimDark=>&[
                ("Saint Verena",0,1.35*AU,6900.0,470000.0),("Reliquary",1,520000.0,2100.0,7500.0),
                ("Cinder",0,0.24*AU,2200.0,18000.0),("Ferrum",0,0.62*AU,5900.0,300000.0),
                ("Penitent",0,2.2*AU,4100.0,90000.0),("Scourge",5,44000.0,220.0,30.0),
                ("Thurible",0,7.4*AU,74000.0,145000000.0),("Vigil",7,390000.0,1500.0,3200.0),
                ("Ashen Choir",7,830000.0,2500.0,11000.0),("Ossuary",7,1900000.0,1700.0,4400.0),
                ("Last Judgement",0,24.0*AU,34000.0,11000000.0),("Pall",11,410000.0,1100.0,1800.0),
                ("Mortis",11,1300000.0,700.0,350.0)],
            Self::Imperium=>&[
                ("Kestrel Prime",0,0.88*AU,6100.0,350000.0),("Portfall",1,310000.0,1500.0,3200.0),
                ("Furnace",0,0.32*AU,3100.0,38000.0),("Dryhaven",0,1.7*AU,4700.0,150000.0),
                ("Prospect",4,98000.0,620.0,240.0),("Warrant",0,4.1*AU,64000.0,95000000.0),
                ("Bond",6,420000.0,2300.0,8000.0),("Ledger",6,920000.0,1800.0,4700.0),
                ("Farpoint",0,13.2*AU,47000.0,24000000.0),("Survey",9,360000.0,900.0,800.0),
                ("Outpost",9,1700000.0,1400.0,2600.0)],
            _=>&[
                ("Lilt",0,1.65*AU,7200.0,510000.0),("Aside",1,680000.0,1900.0,6000.0),
                ("Margin",1,220000.0,950.0,1200.0),("Ember",0,0.49*AU,4400.0,120000.0),
                ("Serein",0,6.2*AU,67000.0,115000000.0),("Interval",5,440000.0,2000.0,6400.0),
                ("Cadence",5,760000.0,2800.0,14000.0),("Counterpoint",5,1500000.0,1400.0,2700.0),
                ("Vellum",0,18.6*AU,39000.0,17000000.0),("Trace",9,340000.0,1100.0,1800.0),
                ("Pale Blue",9,730000.0,1750.0,4500.0),("Afterthought",9,1800000.0,820.0,620.0)],
        };
        let mut rng=Rng::stream(seed,0x53595354454d+self as u64);
        for &(name,parent,distance,radius,gm) in bodies {
            let angle=rng.uniform()*std::f64::consts::TAU;
            let pos=system.state(parent,0.0).pos+Vec2::new(angle.cos(),angle.sin())*distance;
            system.bodies.push(Celestial {name:name.into(),kind:if parent==0 {CelestialKind::Planet} else {CelestialKind::Moon},gm,radius,
                orbit:Orbit::Frozen {parent,radius:distance,pos}});
        }
        system
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinct_repeatable_systems_support_the_escort_mission() {
        for (theme,planets,moons) in [(Theme::Luminal,8,13),(Theme::GrimDark,6,7),(Theme::Imperium,5,6),(Theme::Culture,4,8)] {
            for seed in [7,42,99] {
                let system=theme.star_system(seed);let repeat=theme.star_system(seed);
                assert_eq!(system.bodies.iter().filter(|b|b.kind==CelestialKind::Planet).count(),planets);
                assert_eq!(system.bodies.iter().filter(|b|b.kind==CelestialKind::Moon).count(),moons);
                assert_eq!(system.bodies[1].kind,CelestialKind::Planet);assert_eq!(system.bodies[2].kind,CelestialKind::Moon);
                for (i,b) in system.bodies.iter().enumerate() {
                    assert_eq!(system.state(i,0.0),repeat.state(i,10000.0));
                    if let Orbit::Frozen {parent,radius,..}=b.orbit {assert!(radius>b.radius+system.bodies[parent].radius);}
                }
                let mut world=luminal_core::scenario::transport_intercept_class_in_system(seed,luminal_core::world::ShipClass::Destroyer,system);
                world.advance_to(600.0);
                for id in [0,1,3] {assert!(world.body(luminal_core::world::BodyId(id)).unwrap().alive_at(600.0),"initial escort or station lost in {theme:?}, seed {seed}");}
            }
        }
    }
}
