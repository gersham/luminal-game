//! Ablative armour, hull integrity and discrete subsystem casualties.
use crate::{params::HULL_INTEGRITY_J,rng::Rng};

pub const JOULES_PER_HP:f64=HULL_INTEGRITY_J.value/100.0;
pub const SCREEN_LEAK_CHANCE:f64=0.05;
pub const SCREEN_LEAK_FRACTION:f64=0.01;
pub const SYSTEM_HIT_CHANCE:f64=0.20;
pub const MISSILE_SCREEN_LEAK_FRACTION:f64=0.25;
pub const CRITICAL_MIN_HULL_FRACTION:f64=0.01;
pub const MISSILE_SCREEN_LEAK_CHANCE:f64=0.35;
pub const FRIGATE_HULL_HP:f64=1000.0;
pub const SYSTEM_REPAIR_SECONDS:f64=1200.0;
pub const HULL_REPAIR_SECONDS_PER_PERCENT:f64=3600.0;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Condition {Intact,Damaged,Destroyed}
impl Condition {
    pub fn effectiveness(self)->f64 {match self {Self::Intact=>1.0,Self::Damaged=>0.5,Self::Destroyed=>0.0}}
    pub fn hit(&mut self) {*self=match self {Self::Intact=>Self::Damaged,_=>Self::Destroyed};}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(usize)]
pub enum System {Passive,Active,Direction,Ecm,Eccm,Propulsion,Power,Screens,Crew,Mind,PdMissiles,PdLaser,Beam,Launcher,Repair,SrmLauncher}
impl System {
    pub fn independent_power(self)->bool {
        matches!(self,Self::Crew|Self::Repair|Self::Passive|Self::Direction|Self::Mind)
    }
    pub const ALL:[Self;16]=[Self::Passive,Self::Active,Self::Direction,Self::Ecm,Self::Eccm,Self::Propulsion,Self::Power,Self::Screens,Self::Crew,Self::Mind,Self::PdMissiles,Self::PdLaser,Self::Beam,Self::Launcher,Self::Repair,Self::SrmLauncher];
    pub fn code(self)->&'static str {match self {Self::Passive=>"PASS",Self::Active=>"ACTV",Self::Direction=>"DIRF",Self::Ecm=>"ECMS",Self::Eccm=>"ECCM",Self::Propulsion=>"PROP",Self::Power=>"POWR",Self::Screens=>"SCRN",Self::Crew=>"CREW",Self::Mind=>"MIND",Self::PdMissiles=>"PDMS",Self::PdLaser=>"PDLS",Self::Beam=>"BEAM",Self::Launcher=>"LRM",Self::SrmLauncher=>"SRM",Self::Repair=>"DCTL"}}
    pub fn name(self)->&'static str {match self {Self::Passive=>"Passive sensors",Self::Active=>"Active sensors",Self::Direction=>"Direction finding",Self::Ecm=>"Electronic countermeasures",Self::Eccm=>"Electronic counter-countermeasures",Self::Propulsion=>"Propulsion",Self::Power=>"Power generation",Self::Screens=>"Screens",Self::Crew=>"Crew",Self::Mind=>"Ship mind",Self::PdMissiles=>"Point-defence missile launcher",Self::PdLaser=>"Point-defence laser",Self::Beam=>"Main beam",Self::Launcher=>"LRM launchers",Self::SrmLauncher=>"SRM launchers",Self::Repair=>"Damage control"}}
}
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct Damage {
    pub hull:f64,pub hull_max:f64,pub armour:f64,pub armour_max:f64,
    pub systems:[Condition;16],
    pub repair_progress:f64,
    pub repair_target:Option<System>,
}
impl Default for Damage {fn default()->Self {Self {hull:100.0,hull_max:100.0,armour:100.0,armour_max:100.0,systems:[Condition::Intact;16],repair_progress:0.0,repair_target:None}}}
impl Damage {
    /// Catastrophic field feedback bypasses armour. Each secondary casualty is
    /// distinct, installed, and not already destroyed.
    pub fn screen_overload(&mut self,installed:&[bool;16],rng:&mut Rng)->Vec<System> {
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
    pub fn lifeless(&self)->bool {
        self.state(System::Crew)==Condition::Destroyed && self.state(System::Mind)==Condition::Destroyed
    }
    /// Shared dependency rules for simulation and received damage reports.
    pub fn operating_effectiveness(&self,system:System)->f64 {
        if self.lifeless() {return 0.0;}
        let base=self.effectiveness(system);
        if system==System::Repair {
            return base*self.effectiveness(System::Crew)*self.effectiveness(System::Mind);
        }
        if system.independent_power() {return base;}
        base*self.effectiveness(System::Power)*self.effectiveness(System::Mind)
    }
    /// Every nonzero penetration damages hull. Armour absorbs half until exhausted;
    /// unused absorption flows through, so energy cannot disappear at depletion.
    pub fn penetrate(&mut self,energy_j:f64,installed:&[bool;16],rng:&mut Rng)->Option<System> {
        let points=energy_j.max(0.0)/JOULES_PER_HP;
        let soaked=(points*0.5).min(self.armour);
        self.armour-=soaked;
        self.hull=(self.hull-(points-soaked)).max(0.0);
        if points<self.hull_max*CRITICAL_MIN_HULL_FRACTION || rng.uniform()>=self.system_hit_chance() {return None;}
        self.hit_system(installed,rng)
    }
    pub fn hit_system(&mut self,installed:&[bool;16],rng:&mut Rng)->Option<System> {
        let eligible:Vec<_>=System::ALL.into_iter().filter(|s|installed[*s as usize] && self.state(*s)!=Condition::Destroyed).collect();
        if eligible.is_empty() {return None;}
        let weight=|s:System|if s==System::Propulsion {2.0} else {1.0};
        let mut roll=rng.uniform()*eligible.iter().map(|s|weight(*s)).sum::<f64>();
        let system=eligible.iter().copied().find(|s| {roll-=weight(*s);roll<0.0}).unwrap_or(*eligible.last().unwrap());
        self.systems[system as usize].hit();
        Some(system)
    }
    pub fn system_repair_rate(&self)->f64 {
        (match self.state(System::Repair) {Condition::Intact=>1.0,Condition::Damaged=>1.0/3.0,Condition::Destroyed=>0.0})
            *self.effectiveness(System::Crew)*self.effectiveness(System::Mind)
    }
    pub fn repair(&mut self,dt:f64,rng:&mut Rng)->Option<System> {
        if self.hull<=0.0 || self.state(System::Power)==Condition::Destroyed || self.operating_effectiveness(System::Repair)==0.0 {return None;}
        let work=dt.max(0.0)*self.operating_effectiveness(System::Repair);
        // Emergency damage-control work can restart power without powered systems.
        // Other repair work, including hull restoration, waits for power.
        if self.state(System::Power)==Condition::Intact {
            self.hull=(self.hull+self.hull_max*0.01*work/HULL_REPAIR_SECONDS_PER_PERCENT).min(self.hull_max);
        }
        let damaged:Vec<_>=System::ALL.into_iter().filter(|s|self.state(*s)==Condition::Damaged).collect();
        if damaged.is_empty() {self.repair_progress=0.0;self.repair_target=None;return None;}
        let repaired=if self.state(System::Power)==Condition::Damaged {System::Power}
            else if self.state(System::Repair)==Condition::Damaged {System::Repair}
            else if let Some(target)=self.repair_target.filter(|s|self.state(*s)==Condition::Damaged) {target}
            else {damaged[(rng.uniform()*damaged.len() as f64) as usize]};
        if self.repair_target!=Some(repaired) {self.repair_progress=0.0;self.repair_target=Some(repaired);}
        self.repair_progress+=dt.max(0.0)*self.system_repair_rate();
        if self.repair_progress+1e-8<SYSTEM_REPAIR_SECONDS {return None;}
        self.repair_progress=0.0;
        self.repair_target=None;
        self.systems[repaired as usize]=Condition::Intact;
        Some(repaired)
    }
}
#[derive(Clone,Copy,Debug)]
pub struct Report {pub damage:Damage,pub installed:[bool;16],pub observed_at:f64,pub screen_available:f64}
impl Report {
    pub fn operating_effectiveness(&self,system:System)->f64 {
        if !self.installed[system as usize] {return 0.0;}
        self.damage.operating_effectiveness(system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn dead_mind_stops_repairs_and_dead_crew_and_mind_disable_everything() {
        let mut d=Damage {hull:40.0,..Default::default()};
        d.systems[System::Mind as usize]=Condition::Destroyed;
        d.systems[System::Active as usize]=Condition::Damaged;
        for s in System::ALL {
            assert_eq!(d.operating_effectiveness(s),if matches!(s,System::Crew|System::Passive|System::Direction) {1.0} else {0.0},"{s:?}");
        }
        d.repair(86400.0,&mut Rng::new(1));assert_eq!(d.hull,40.0);
        assert_eq!(d.state(System::Active),Condition::Damaged);assert_eq!(d.repair_progress,0.0);
        d.systems[System::Crew as usize]=Condition::Destroyed;
        assert!(d.lifeless());
        let report=Report {damage:d,installed:[true;16],observed_at:0.0,screen_available:0.0};
        for s in System::ALL {assert_eq!(report.operating_effectiveness(s),0.0,"{s:?}");}
        d.systems[System::Mind as usize]=Condition::Intact;
        assert!(!d.lifeless());assert_eq!(d.operating_effectiveness(System::Beam),1.0);
        assert_eq!(d.operating_effectiveness(System::Repair),0.0,"repair still needs living crew");
    }
    #[test] fn screen_overload_destroys_generator_hits_distinct_systems_and_bypasses_armour() {
        for seed in 0..100 {
            let mut d=Damage {hull:1000.0,hull_max:1000.0,..Default::default()};
            let hit=d.screen_overload(&[true;16],&mut Rng::new(seed));
            assert!((2..=4).contains(&hit.len()));
            assert_eq!(d.hull,800.0);assert_eq!(d.armour,100.0);
            assert_eq!(d.state(System::Screens),Condition::Destroyed);
            for (i,s) in hit.iter().enumerate() {assert!(!hit[..i].contains(s));}
        }
        let mut d=Damage {hull:100.0,hull_max:1000.0,..Default::default()};
        let mut installed=[false;16];installed[System::Beam as usize]=true;
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
                // Restore the qualifying penetration's hull loss so the tested post-hit
                // condition lies exactly on each boundary.
                d.hull+=5.0;
                d.penetrate(10.0*JOULES_PER_HP,&[true;16],&mut rng).is_some()
            }).count();
            assert!((hits as f64/20_000.0-chance).abs()<0.02,"{hull}: {hits}");
        }
        let mut d=Damage {hull:501.0,hull_max:1000.0,armour:0.0,..Default::default()};
        d.penetrate(3.0*JOULES_PER_HP,&[false;16],&mut Rng::new(1));
        assert_eq!(d.system_hit_chance(),0.4,"the hit crossing the threshold uses the higher chance");
    }
    #[test] fn repair_target_is_stable_and_completes_after_twenty_minutes() {
        let mut d=Damage::default();
        let mut rng=Rng::new(12);
        for s in [System::Passive,System::Active] {d.systems[s as usize]=Condition::Damaged;}
        assert_eq!(d.repair(600.0,&mut rng),None);
        let target=d.repair_target.unwrap();
        assert_eq!(d.repair_progress/SYSTEM_REPAIR_SECONDS,0.5);
        assert_eq!(d.repair(599.0,&mut rng),None);
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
        let report=Report {damage:d,installed:[true;16],observed_at:0.0,screen_available:1.0};
        for s in System::ALL {
            let expected=if s==System::Repair {0.5} else if s.independent_power() {1.0} else {0.0};
            assert_eq!(report.operating_effectiveness(s),expected,"{s:?}");
        }
        let mut rng=Rng::new(42);
        assert_eq!(d.repair(3600.0,&mut rng),Some(System::Power));
        assert_eq!(d.hull,50.0);
        assert_eq!(d.state(System::Repair),Condition::Damaged);
        assert_eq!(d.repair(3600.0,&mut rng),Some(System::Repair));
        assert!(d.hull>50.0);
    }
    #[test] fn hull_repairs_one_percent_per_hour_without_repairing_armour() {
        let mut d=Damage {hull:400.0,hull_max:500.0,armour:50.0,..Default::default()};
        let mut rng=Rng::new(8);
        d.repair(3600.0,&mut rng);
        assert_eq!(d.hull,405.0);assert_eq!(d.armour,50.0);
        d.systems[System::Repair as usize]=Condition::Damaged;
        d.repair(3600.0,&mut rng);assert_eq!(d.hull,407.5);
    }
    #[test] fn penetrating_hits_damage_systems_twenty_percent_of_the_time() {
        let mut rng=Rng::new(314);
        let hits=(0..20_000).filter(|_|Damage::default().penetrate(JOULES_PER_HP,&[true;16],&mut rng).is_some()).count();
        assert!((3800..4200).contains(&hits),"{hits}/20000");
        assert_eq!(MISSILE_SCREEN_LEAK_CHANCE,0.35);
    }
    #[test] fn grazing_hits_cannot_destroy_subsystems() {
        let mut d=Damage {hull:8000.0,hull_max:8000.0,armour:6000.0,..Default::default()};
        let mut rng=Rng::new(91);
        for _ in 0..10_000 {assert!(d.penetrate(1.05e11,&[true;16],&mut rng).is_none());}
        assert!(d.systems.iter().all(|s|*s==Condition::Intact));
    }
    #[test] fn propulsion_has_double_weight_and_power_has_normal_weight() {
        let mut rng=Rng::new(42);
        let mut installed=[false;16];
        for s in [System::Propulsion,System::Power,System::Active] {installed[s as usize]=true;}
        let mut counts=[0;16];
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
        d.penetrate(20.0*JOULES_PER_HP,&[false;16],&mut rng);
        assert_eq!((d.hull,d.armour),(90.0,90.0));
        d.armour=3.0;d.penetrate(20.0*JOULES_PER_HP,&[false;16],&mut rng);
        assert_eq!((d.hull,d.armour),(73.0,0.0));
        d.penetrate(100.0*JOULES_PER_HP,&[false;16],&mut rng);
        assert_eq!(d.hull,0.0);
    }
    #[test] fn two_system_hits_destroy_without_system_hitpoints() {
        let mut c=Condition::Intact;c.hit();assert_eq!(c.effectiveness(),0.5);
        c.hit();assert_eq!(c.effectiveness(),0.0);c.hit();assert_eq!(c,Condition::Destroyed);
    }
    #[test] fn damaged_control_repairs_itself_in_sixty_minutes_then_others_in_twenty() {
        let mut d=Damage {hull:40.0,armour:0.0,..Default::default()};let mut rng=Rng::new(7);
        d.systems[System::Repair as usize]=Condition::Damaged;
        d.systems[System::Passive as usize]=Condition::Damaged;
        d.systems[System::Beam as usize]=Condition::Destroyed;
        for _ in 0..3599 {assert_eq!(d.repair(1.0,&mut rng),None);}
        assert_eq!(d.repair(1.0,&mut rng),Some(System::Repair));
        assert!((d.hull-40.5).abs()<1e-8);
        for _ in 0..1199 {assert_eq!(d.repair(1.0,&mut rng),None);}
        assert_eq!(d.repair(1.0,&mut rng),Some(System::Passive));
        assert!((d.hull-(40.5+1.0/3.0)).abs()<1e-8);
        assert_eq!(d.armour,0.0);assert_eq!(d.state(System::Beam),Condition::Destroyed);
    }
    #[test] fn disabled_repairs_and_dead_hulls_cannot_regenerate() {
        let mut d=Damage {hull:40.0,..Default::default()};let mut rng=Rng::new(1);
        d.systems[System::Repair as usize]=Condition::Destroyed;
        d.repair(3600.0,&mut rng);assert_eq!(d.hull,40.0);
        d=Damage {hull:0.0,..Default::default()};d.repair(3600.0,&mut rng);assert_eq!(d.hull,0.0);
    }
}
