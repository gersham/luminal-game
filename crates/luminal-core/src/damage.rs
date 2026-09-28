//! Ablative armour, hull integrity and discrete subsystem casualties.
use crate::{params::HULL_INTEGRITY_J,rng::Rng};

pub const JOULES_PER_HP:f64=HULL_INTEGRITY_J.value/100.0;
pub const SCREEN_LEAK_CHANCE:f64=0.05;
pub const SCREEN_LEAK_FRACTION:f64=0.01;
pub const SYSTEM_HIT_CHANCE:f64=0.20;
pub const MISSILE_SCREEN_COUPLING:f64=0.4;
pub const MISSILE_SCREEN_LEAK_CHANCE:f64=0.35;
pub const FRIGATE_HULL_HP:f64=1000.0;
pub const SYSTEM_REPAIR_SECONDS:f64=120.0;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Condition {Intact,Damaged,Destroyed}
impl Condition {
    pub fn effectiveness(self)->f64 {match self {Self::Intact=>1.0,Self::Damaged=>0.5,Self::Destroyed=>0.0}}
    pub fn hit(&mut self) {*self=match self {Self::Intact=>Self::Damaged,_=>Self::Destroyed};}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(usize)]
pub enum System {Passive,Active,Direction,Ecm,Eccm,Propulsion,Power,Screens,Crew,Mind,PdMissiles,PdLaser,Beam,Launcher,Repair}
impl System {
    pub fn independent_power(self)->bool {
        matches!(self,Self::Crew|Self::Repair|Self::Passive|Self::Direction|Self::Mind)
    }
    pub const ALL:[Self;15]=[Self::Passive,Self::Active,Self::Direction,Self::Ecm,Self::Eccm,Self::Propulsion,Self::Power,Self::Screens,Self::Crew,Self::Mind,Self::PdMissiles,Self::PdLaser,Self::Beam,Self::Launcher,Self::Repair];
    pub fn code(self)->&'static str {match self {Self::Passive=>"PASS",Self::Active=>"ACTV",Self::Direction=>"DIRF",Self::Ecm=>"ECMS",Self::Eccm=>"ECCM",Self::Propulsion=>"PROP",Self::Power=>"POWR",Self::Screens=>"SCRN",Self::Crew=>"CREW",Self::Mind=>"MIND",Self::PdMissiles=>"PDMS",Self::PdLaser=>"PDLS",Self::Beam=>"BEAM",Self::Launcher=>"LNCH",Self::Repair=>"DCTL"}}
    pub fn name(self)->&'static str {match self {Self::Passive=>"Passive sensors",Self::Active=>"Active sensors",Self::Direction=>"Direction finding",Self::Ecm=>"Electronic countermeasures",Self::Eccm=>"Electronic counter-countermeasures",Self::Propulsion=>"Propulsion",Self::Power=>"Power generation",Self::Screens=>"Screens",Self::Crew=>"Crew",Self::Mind=>"Ship mind",Self::PdMissiles=>"Point-defence missile launcher",Self::PdLaser=>"Point-defence laser",Self::Beam=>"Main beam",Self::Launcher=>"Offensive missile launcher",Self::Repair=>"Damage control"}}
}
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct Damage {
    pub hull:f64,pub hull_max:f64,pub armour:f64,pub armour_max:f64,
    pub systems:[Condition;15],
    pub repair_progress:f64,
    pub repair_target:Option<System>,
}
impl Default for Damage {fn default()->Self {Self {hull:100.0,hull_max:100.0,armour:100.0,armour_max:100.0,systems:[Condition::Intact;15],repair_progress:0.0,repair_target:None}}}
impl Damage {
    /// Catastrophic field feedback bypasses armour. Each secondary casualty is
    /// distinct, installed, and not already destroyed.
    pub fn screen_overload(&mut self,installed:&[bool;15],rng:&mut Rng)->Vec<System> {
        self.systems[System::Screens as usize]=Condition::Destroyed;
        self.hull=(self.hull-0.2*self.hull_max).max(0.0);
        let mut eligible=*installed;
        eligible[System::Screens as usize]=false;
        let count=1+(rng.uniform()*3.0) as usize;
        let mut hit=vec![System::Screens];
        for _ in 0..count {
            if let Some(s)=self.hit_system(&eligible,rng) {eligible[s as usize]=false;hit.push(s);}
        }
        hit
    }
    pub fn hull_thrust_factor(&self)->f64 {
        let fraction=self.hull/self.hull_max.max(1e-9);
        if fraction<=0.25 {0.5} else if fraction<=0.5 {0.75} else {1.0}
    }
    pub fn system_hit_chance(&self)->f64 {
        let fraction=self.hull/self.hull_max.max(1e-9);
        SYSTEM_HIT_CHANCE*if fraction<0.25 {4.0} else if fraction<0.5 {2.0} else {1.0}
    }
    pub fn state(&self,system:System)->Condition {
        self.systems[system as usize]
    }
    pub fn effectiveness(&self,system:System)->f64 {
        if system==System::Power && self.state(system)!=Condition::Intact {0.0}
        else {self.state(system).effectiveness()}
    }
    /// Every nonzero penetration damages hull. Armour absorbs half until exhausted;
    /// unused absorption flows through, so energy cannot disappear at depletion.
    pub fn penetrate(&mut self,energy_j:f64,installed:&[bool;15],rng:&mut Rng)->Option<System> {
        let points=energy_j.max(0.0)/JOULES_PER_HP;
        let soaked=(points*0.5).min(self.armour);
        self.armour-=soaked;
        self.hull=(self.hull-(points-soaked)).max(0.0);
        if energy_j<1e6 || rng.uniform()>=self.system_hit_chance() {return None;}
        self.hit_system(installed,rng)
    }
    pub fn hit_system(&mut self,installed:&[bool;15],rng:&mut Rng)->Option<System> {
        let eligible:Vec<_>=System::ALL.into_iter().filter(|s|installed[*s as usize] && self.state(*s)!=Condition::Destroyed).collect();
        if eligible.is_empty() {return None;}
        let weight=|s:System|if s==System::Propulsion {2.0} else {1.0};
        let mut roll=rng.uniform()*eligible.iter().map(|s|weight(*s)).sum::<f64>();
        let system=eligible.iter().copied().find(|s| {roll-=weight(*s);roll<0.0}).unwrap_or(*eligible.last().unwrap());
        self.systems[system as usize].hit();
        Some(system)
    }
    pub fn repair(&mut self,dt:f64,rng:&mut Rng)->Option<System> {
        if self.hull<=0.0 || self.state(System::Power)==Condition::Destroyed {return None;}
        let work=dt.max(0.0)*self.effectiveness(System::Repair)*self.effectiveness(System::Crew);
        // Emergency damage-control work can restart power without powered systems.
        // Other repair work, including hull restoration, waits for power.
        if self.state(System::Power)==Condition::Intact {
            self.hull=(self.hull+self.hull_max*0.01*work/600.0).min(self.hull_max);
        }
        let damaged:Vec<_>=System::ALL.into_iter().filter(|s|self.state(*s)==Condition::Damaged).collect();
        if damaged.is_empty() {self.repair_progress=0.0;self.repair_target=None;return None;}
        let repaired=if self.state(System::Power)==Condition::Damaged {System::Power}
            else if self.state(System::Repair)==Condition::Damaged {System::Repair}
            else if let Some(target)=self.repair_target.filter(|s|self.state(*s)==Condition::Damaged) {target}
            else {damaged[(rng.uniform()*damaged.len() as f64) as usize]};
        if self.repair_target!=Some(repaired) {self.repair_progress=0.0;self.repair_target=Some(repaired);}
        self.repair_progress+=work;
        if self.repair_progress<SYSTEM_REPAIR_SECONDS {return None;}
        self.repair_progress=0.0;
        self.repair_target=None;
        self.systems[repaired as usize]=Condition::Intact;
        Some(repaired)
    }
}
#[derive(Clone,Copy,Debug)]
pub struct Report {pub damage:Damage,pub installed:[bool;15],pub observed_at:f64,pub screen_heat:f64}
impl Report {
    pub fn operating_effectiveness(&self,system:System)->f64 {
        if !self.installed[system as usize] {return 0.0;}
        if system.independent_power() {return self.damage.effectiveness(system);}
        self.damage.effectiveness(system)*self.damage.effectiveness(System::Power)*self.damage.effectiveness(System::Mind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn screen_overload_destroys_generator_hits_distinct_systems_and_bypasses_armour() {
        for seed in 0..100 {
            let mut d=Damage {hull:1000.0,hull_max:1000.0,..Default::default()};
            let hit=d.screen_overload(&[true;15],&mut Rng::new(seed));
            assert!((2..=4).contains(&hit.len()));
            assert_eq!(d.hull,800.0);assert_eq!(d.armour,100.0);
            assert_eq!(d.state(System::Screens),Condition::Destroyed);
            for (i,s) in hit.iter().enumerate() {assert!(!hit[..i].contains(s));}
        }
        let mut d=Damage {hull:100.0,hull_max:1000.0,..Default::default()};
        let mut installed=[false;15];installed[System::Beam as usize]=true;
        d.systems[System::Beam as usize]=Condition::Damaged;
        let hit=d.screen_overload(&installed,&mut Rng::new(1));
        assert_eq!(hit,vec![System::Screens,System::Beam]);
        assert_eq!(d.state(System::Beam),Condition::Destroyed);assert_eq!(d.hull,0.0);
    }
    #[test] fn system_damage_chance_increases_below_hull_thresholds() {
        for (hull,chance) in [(1000.0,0.2),(500.0,0.2),(499.0,0.4),(250.0,0.4),(249.0,0.8)] {
            let damage=Damage {hull,hull_max:1000.0,..Default::default()};
            assert_eq!(damage.system_hit_chance(),chance);
            let mut rng=Rng::new(31);
            let hits=(0..20_000).filter(|_| {
                let mut d=damage;
                // Restore the tiny penetration's hull loss so the tested post-hit
                // condition lies exactly on each boundary.
                d.hull+=1e9/JOULES_PER_HP*0.5;
                d.penetrate(1e9,&[true;15],&mut rng).is_some()
            }).count();
            assert!((hits as f64/20_000.0-chance).abs()<0.02,"{hull}: {hits}");
        }
        let mut d=Damage {hull:501.0,hull_max:1000.0,armour:0.0,..Default::default()};
        d.penetrate(3.0*JOULES_PER_HP,&[false;15],&mut Rng::new(1));
        assert_eq!(d.system_hit_chance(),0.4,"the hit crossing the threshold uses the higher chance");
    }
    #[test] fn repair_target_is_stable_and_completes_after_two_minutes() {
        let mut d=Damage::default();
        let mut rng=Rng::new(12);
        for s in [System::Passive,System::Active] {d.systems[s as usize]=Condition::Damaged;}
        assert_eq!(d.repair(60.0,&mut rng),None);
        let target=d.repair_target.unwrap();
        assert_eq!(d.repair_progress/SYSTEM_REPAIR_SECONDS,0.5);
        assert_eq!(d.repair(59.0,&mut rng),None);
        assert_eq!(d.repair_target,Some(target));
        assert_eq!(d.repair(1.0,&mut rng),Some(target));
        assert_eq!(d.state(target),Condition::Intact);
        assert_eq!(d.repair_target,None);
        assert_eq!(d.repair_progress,0.0);
    }
    #[test] fn emergency_repairs_restore_power_before_damage_control() {
        let mut d=Damage {hull:50.0,..Default::default()};
        d.systems[System::Power as usize]=Condition::Damaged;
        d.systems[System::Repair as usize]=Condition::Damaged;
        let report=Report {damage:d,installed:[true;15],observed_at:0.0,screen_heat:0.0};
        for s in System::ALL {
            let expected=if s==System::Repair {0.5} else if s.independent_power() {1.0} else {0.0};
            assert_eq!(report.operating_effectiveness(s),expected,"{s:?}");
        }
        let mut rng=Rng::new(42);
        assert_eq!(d.repair(240.0,&mut rng),Some(System::Power));
        assert_eq!(d.hull,50.0);
        assert_eq!(d.state(System::Repair),Condition::Damaged);
        assert_eq!(d.repair(240.0,&mut rng),Some(System::Repair));
        assert!(d.hull>50.0);
    }
    #[test] fn hull_repairs_one_percent_per_ten_minutes_without_repairing_armour() {
        let mut d=Damage {hull:400.0,hull_max:500.0,armour:50.0,..Default::default()};
        let mut rng=Rng::new(8);
        d.repair(600.0,&mut rng);
        assert_eq!(d.hull,405.0);assert_eq!(d.armour,50.0);
        d.systems[System::Repair as usize]=Condition::Damaged;
        d.repair(600.0,&mut rng);assert_eq!(d.hull,407.5);
    }
    #[test] fn penetrating_hits_damage_systems_twenty_percent_of_the_time() {
        let mut rng=Rng::new(314);
        let hits=(0..20_000).filter(|_|Damage::default().penetrate(1e9,&[true;15],&mut rng).is_some()).count();
        assert!((3800..4200).contains(&hits),"{hits}/20000");
        assert_eq!(MISSILE_SCREEN_LEAK_CHANCE,0.35);
    }
    #[test] fn propulsion_has_double_weight_and_power_has_normal_weight() {
        let mut rng=Rng::new(42);
        let mut installed=[false;15];
        for s in [System::Propulsion,System::Power,System::Active] {installed[s as usize]=true;}
        let mut counts=[0;15];
        for _ in 0..20_000 {
            let mut damage=Damage::default();
            let system=damage.hit_system(&installed,&mut rng).unwrap();
            counts[system as usize]+=1;
        }
        let normal=counts[System::Active as usize] as f64;
        let ratio=counts[System::Propulsion as usize] as f64/normal;
        assert!((1.85..2.15).contains(&ratio),"propulsion: {ratio}");
        assert!((0.9..1.1).contains(&(counts[System::Power as usize] as f64/normal)));
        let mut damage=Damage::default();
        damage.systems[System::Power as usize]=Condition::Destroyed;
        for _ in 0..10 {assert_ne!(damage.hit_system(&installed,&mut rng),Some(System::Power));}
    }
    #[test] fn armour_ablates_and_all_penetrations_damage_hull() {
        let mut d=Damage::default();let mut rng=Rng::new(1);
        d.penetrate(20.0*JOULES_PER_HP,&[false;15],&mut rng);
        assert_eq!((d.hull,d.armour),(90.0,90.0));
        d.armour=3.0;d.penetrate(20.0*JOULES_PER_HP,&[false;15],&mut rng);
        assert_eq!((d.hull,d.armour),(73.0,0.0));
        d.penetrate(100.0*JOULES_PER_HP,&[false;15],&mut rng);
        assert_eq!(d.hull,0.0);
    }
    #[test] fn two_system_hits_destroy_without_system_hitpoints() {
        let mut c=Condition::Intact;c.hit();assert_eq!(c.effectiveness(),0.5);
        c.hit();assert_eq!(c.effectiveness(),0.0);c.hit();assert_eq!(c,Condition::Destroyed);
    }
    #[test] fn damage_control_repairs_itself_first_at_half_rate() {
        let mut d=Damage {hull:40.0,armour:0.0,..Default::default()};let mut rng=Rng::new(7);
        d.systems[System::Repair as usize]=Condition::Damaged;
        d.systems[System::Passive as usize]=Condition::Damaged;
        d.systems[System::Beam as usize]=Condition::Destroyed;
        for _ in 0..239 {assert_eq!(d.repair(1.0,&mut rng),None);}
        assert_eq!(d.repair(1.0,&mut rng),Some(System::Repair));
        assert!((d.hull-40.2).abs()<1e-8);
        for _ in 0..119 {assert_eq!(d.repair(1.0,&mut rng),None);}
        assert_eq!(d.repair(1.0,&mut rng),Some(System::Passive));
        assert!((d.hull-40.4).abs()<1e-8);
        assert_eq!(d.armour,0.0);assert_eq!(d.state(System::Beam),Condition::Destroyed);
    }
    #[test] fn disabled_repairs_and_dead_hulls_cannot_regenerate() {
        let mut d=Damage {hull:40.0,..Default::default()};let mut rng=Rng::new(1);
        d.systems[System::Repair as usize]=Condition::Destroyed;
        d.repair(3600.0,&mut rng);assert_eq!(d.hull,40.0);
        d=Damage {hull:0.0,..Default::default()};d.repair(3600.0,&mut rng);assert_eq!(d.hull,0.0);
    }
}
