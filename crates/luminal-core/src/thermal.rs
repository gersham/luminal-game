//! Conservative prototype power and field model. All tuning is in params.
use crate::params::*;

#[derive(Clone, Copy, Debug)]
pub struct Thermal {
    pub capacity_scale:f64,
    pub capacitor_multiplier:f64,
    pub capacitor_j: f64,
    pub heat_j: f64,
    /// Five-second exponential average of drive, weapon and absorbed-screen heat input, W.
    pub heating_w:f64,
    /// Current continuous input, for actual radiation when the reservoir is empty.
    pub continuous_input_w:f64,
    pub dumping: bool,
    pub field: f64,
    pub generated_j: f64,
    pub radiated_j: f64,
    pub captured_j: f64,
    pub emitted_j: f64,
    pub last_t: f64,
}

impl Default for Thermal {
    fn default() -> Self {
        Self { capacity_scale:1.0,capacitor_multiplier:1.0,capacitor_j: BEAM_CAPACITOR_J.value, heat_j: 0.0, heating_w:0.0, continuous_input_w:0.0, dumping: false, field: 0.0,
            generated_j: 0.0, radiated_j: 0.0, captured_j: 0.0, emitted_j: 0.0, last_t: 0.0 }
    }
}

impl Thermal {
    pub fn capacitor_capacity(&self)->f64 {BEAM_CAPACITOR_J.value*self.capacity_scale*self.capacitor_multiplier}
    pub fn cooling_rate(&self)->f64 {if self.dumping {HEAT_DUMP_RATE} else {1.0}}
    pub fn heat_fraction(&self)->f64 {self.heat_j/(SHIP_HEAT_LIMIT_J*self.capacity_scale)}
    /// Full thrust through half a tank, then a straight fall to zero at the limit.
    pub fn thrust_factor(&self)->f64 {(1.0-2.0*(self.heat_fraction()-0.5).max(0.0)).clamp(0.0,1.0)}
    pub fn at_fraction(heat_fraction:f64,capacity_scale:f64)->Self {
        Self {capacity_scale,heat_j:heat_fraction*SHIP_HEAT_LIMIT_J*capacity_scale,..Self::default()}
    }
    /// Seconds of rated thrust until the tank is full. `Some(0)` at or past the limit.
    pub fn rated_burn_left(&self)->Option<f64> {self.time_to_throttle(Self::drive_power(1.0)*self.capacity_scale)}
    pub fn beam_waste(energy:f64)->f64 {energy*(1.0/BEAM_EFFICIENCY.value-1.0)*beam_waste_multiplier()}
    pub fn drive_power(fraction:f64)->f64 {
        0.5*SHIP_HEAT_LIMIT_J/(HULL_COOLING_S.value*(1.0-(-HEAT_FULL_BURN_S/HULL_COOLING_S.value).exp()))/(1.0-HEAT_BALANCED_THRUST.powi(2)-SCREEN_IDLE_HEAT_FRACTION)*fraction.max(0.0).powi(2)
    }
    pub fn baseline_cooling(&self)->f64 {
        (Self::drive_power(HEAT_BALANCED_THRUST)+Self::drive_power(1.0)*SCREEN_IDLE_HEAT_FRACTION)*self.capacity_scale
    }
    pub fn emission(&self) -> f64 {
        let capacity=(self.baseline_cooling()+self.heat_j/HULL_COOLING_S.value)*self.cooling_rate();
        if self.heat_j>0.0 {capacity} else {capacity.min(self.continuous_input_w)}
    }
    pub fn net_heat_flow(&self)->f64 {
        if self.heat_j<=0.0 {(self.continuous_input_w-self.emission()).max(0.0)} else {self.heating_w-self.emission()}
    }
    pub fn signature_multiplier(&self)->f64 {(1.0+self.heat_fraction())*if self.dumping {HEAT_DUMP_SIGNATURE} else {1.0}}
    pub fn time_to_throttle(&self,drive_w:f64)->Option<f64> {
        if self.heat_j>=SHIP_HEAT_LIMIT_J*self.capacity_scale {return Some(0.0);}
        let tau=HULL_COOLING_S.value/self.cooling_rate();
        let equilibrium=(drive_w-self.baseline_cooling()*self.cooling_rate())*tau;
        if equilibrium<=SHIP_HEAT_LIMIT_J*self.capacity_scale {None}
        else {Some(tau*((equilibrium-self.heat_j)/(equilibrium-SHIP_HEAT_LIMIT_J*self.capacity_scale)).ln())}
    }
    pub fn sustainable_thrust_fraction(&self)->f64 {
        ((SHIP_HEAT_LIMIT_J/HULL_COOLING_S.value*self.cooling_rate()
            +self.baseline_cooling()/self.capacity_scale*self.cooling_rate()
            -Self::drive_power(1.0)*SCREEN_IDLE_HEAT_FRACTION)/Self::drive_power(1.0)).max(0.0).sqrt().min(1.0)
    }
    pub fn absorb(&mut self,joules:f64) {
        self.heat_j+=joules;self.captured_j+=joules;self.heating_w+=joules/5.0;
    }
    pub fn add_waste_heat(&mut self,joules:f64) {
        self.heat_j+=joules;
        self.heating_w+=joules/5.0;
        self.generated_j+=joules;
    }
    pub fn advance(&mut self, t: f64, up: bool) {
        self.advance_scaled(t,up,1.0,1.0);
    }
    pub fn advance_scaled(&mut self,t:f64,up:bool,power:f64,screen:f64) {
        self.advance_with_drive(t,up,power,screen,0.0);
    }
    pub fn advance_with_drive(&mut self,t:f64,up:bool,power:f64,screen:f64,drive_w:f64) {
        if !up {self.field=0.0;} else {self.field=self.field.min(screen);}
        let screen_w=if up {Self::drive_power(1.0)*SCREEN_IDLE_HEAT_FRACTION*self.capacity_scale*screen} else {0.0};
        let drive_w=drive_w+screen_w;
        self.continuous_input_w=drive_w;
        let mut remaining = (t - self.last_t).max(0.0);
        while remaining > 1e-8 {
            let dt = remaining.min(1.0);
            remaining -= dt;
            let recharge = (self.capacitor_capacity() - self.capacitor_j).min(REACTOR_W.value*self.capacity_scale*self.capacitor_multiplier * power * dt).max(0.0);
            self.capacitor_j += recharge;
            self.generated_j += recharge;
            if up {
                self.field = (self.field + dt / SCREEN_BUILD_TIME_S.value).min(screen);
            }
            let rate_decay=(-dt/5.0).exp();
            self.heating_w=self.heating_w*rate_decay+drive_w*(1.0-rate_decay);
            let tau=HULL_COOLING_S.value/self.cooling_rate();
            let decay=(-dt/tau).exp();
            let input=drive_w*dt;
            let net_input=drive_w-self.baseline_cooling()*self.cooling_rate();
            let after=(self.heat_j*decay+net_input*tau*(1.0-decay)).max(0.0);
            self.generated_j+=input;
            self.radiated_j+=(self.heat_j+input-after).max(0.0);
            self.heat_j=after;

        }
        self.last_t = self.last_t.max(t);
    }
    pub fn can_fire(&self) -> bool {self.can_fire_energy(SHIP_BEAM_ENERGY_J.value)}
    pub fn can_fire_energy(&self,energy:f64)->bool {
        !self.dumping && self.capacitor_j >= energy / BEAM_EFFICIENCY.value
            && self.heat_j + Self::beam_waste(energy) <= BEAM_HEAT_LIMIT_J.value*self.capacity_scale
    }
    pub fn fire(&mut self) {self.fire_energy(SHIP_BEAM_ENERGY_J.value);}
    pub fn fire_energy(&mut self,energy:f64) {
        let input = energy / BEAM_EFFICIENCY.value;
        let raw = input - energy;
        let waste = raw * beam_waste_multiplier();
        self.capacitor_j -= input;
        self.heat_j += waste;
        self.heating_w += waste / 5.0;
        // The capacitor already paid `raw`. The rest of the scaled waste is new heat.
        self.generated_j += waste - raw;
        self.emitted_j += energy;
    }
    pub fn balance_error(&self) -> f64 {
        self.capacitor_capacity() + self.generated_j + self.captured_j
            - self.capacitor_j - self.heat_j - self.radiated_j - self.emitted_j
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shields_generate_idle_heat_and_all_hit_heat_uses_master_reservoir() {
        let mut thermal=Thermal::default();
        thermal.advance(30000.0,true);assert!((thermal.field-1.0).abs()<1e-9);
        assert!(thermal.heating_w>0.0 && thermal.heat_j==0.0);
        let before=thermal.heat_j;let ef=thermal.signature_multiplier();
        thermal.absorb(1e15);assert_eq!(thermal.heat_j,before+1e15);
        assert!(thermal.signature_multiplier()>ef);
        thermal.advance(30000.0,false);assert_eq!(thermal.field,0.0);assert_eq!(thermal.heat_j,before+1e15);
        thermal.advance(30060.0,true);assert!((thermal.field-0.002).abs()<1e-9);
        assert!(thermal.balance_error().abs()<1e3);
        thermal.advance(30660.0,false);assert!(thermal.heating_w<1.0);
        assert!(thermal.balance_error().abs()<1e3);
    }
    #[test]
    fn every_class_balances_half_thrust_with_screens_and_cools_existing_heat() {
        for class in crate::world::ShipClass::COMBAT.into_iter().chain([crate::world::ShipClass::Transport]) {
            for fraction in [0.0,0.1,0.49,0.5] {
                let mut cold=Thermal {capacity_scale:class.scale(),capacitor_j:BEAM_CAPACITOR_J.value*class.scale(),..Default::default()};
                let mut hot=cold;hot.add_waste_heat(SHIP_HEAT_LIMIT_J*class.scale()*0.5);
                let drive=Thermal::drive_power(fraction)*class.scale();
                cold.advance_with_drive(86_400.0,true,1.0,1.0,drive);
                hot.advance_with_drive(3600.0,true,1.0,1.0,drive);
                assert_eq!(cold.heat_j,0.0,"{class:?} at {fraction}");
                assert_eq!(cold.net_heat_flow(),0.0);
                assert!(hot.heat_fraction()<0.5,"{class:?} must cool while cruising");
                assert!(!cold.dumping && cold.field==1.0);
                assert!(cold.balance_error().abs()<1e7 && hot.balance_error().abs()<1e7);
            }
            let mut high=Thermal {capacity_scale:class.scale(),capacitor_j:BEAM_CAPACITOR_J.value*class.scale(),..Default::default()};
            high.advance_with_drive(3600.0,true,1.0,1.0,Thermal::drive_power(1.0)*class.scale());
            assert!(high.heat_fraction()>0.5 && high.heat_fraction()<0.52);
        }
    }
    #[test]
    fn full_burn_heat_budget_is_preserved_and_remains_quadratic() {
        assert!((Thermal::drive_power(1.0)/Thermal::drive_power(0.1)-100.0).abs()<1e-10);
        let mut thermal=Thermal::default();
        let power=Thermal::drive_power(1.0);
        let threshold=thermal.time_to_throttle(power).unwrap();
        assert!(threshold>3.0*3600.0 && threshold<3.2*3600.0);
        thermal.advance_with_drive(3599.0,false,1.0,1.0,power);
        assert_eq!(thermal.thrust_factor(),1.0);
        thermal.advance_with_drive(3600.0,false,1.0,1.0,power);
        assert!((thermal.heat_fraction()-0.5).abs()<1e-10);
        // The one-second steps land a hair over half a tank. The curve itself is full thrust at exactly half.
        assert!((thermal.thrust_factor()-1.0).abs()<1e-9,"a one-hour burn is still full thrust");
        assert_eq!(Thermal::at_fraction(0.5,1.0).thrust_factor(),1.0,"the fall starts at half a tank");
        assert!((Thermal::at_fraction(0.75,1.0).thrust_factor()-0.5).abs()<1e-12);
        assert_eq!(Thermal::at_fraction(1.0,1.0).thrust_factor(),0.0);
        assert_eq!(Thermal::at_fraction(1.25,1.0).thrust_factor(),0.0);
        thermal.advance_with_drive(threshold+400.0,false,1.0,1.0,power);
        assert_eq!(thermal.thrust_factor(),0.0);
        assert!(thermal.balance_error().abs()<1e5);
        let cold=Thermal::default().rated_burn_left().unwrap();
        let half=Thermal::at_fraction(0.5,1.0).rated_burn_left().unwrap();
        assert!(cold>3.0*3600.0 && cold<3.2*3600.0);
        assert!(half>0.0 && half<cold);
        assert_eq!(Thermal::at_fraction(1.0,1.0).rated_burn_left(),Some(0.0));
        assert_eq!(Thermal::at_fraction(1.2,2.0).rated_burn_left(),Some(0.0));
    }
    #[test]
    fn dump_radiates_faster_stays_conservative_and_remains_on() {
        let mut normal=Thermal::default();normal.add_waste_heat(SHIP_HEAT_LIMIT_J);
        let mut dump=normal;dump.dumping=true;
        assert!((dump.emission()/normal.emission()-5.0).abs()<1e-10);
        assert!(dump.signature_multiplier()>9.0*normal.signature_multiplier());
        normal.advance(120.0,false);dump.advance(120.0,false);
        assert!(dump.heat_j<normal.heat_j*0.95);
        assert!(dump.balance_error().abs()<1e5);
        dump.advance(6000.0,false);
        assert!(dump.dumping && dump.heat_fraction()<=0.05);
        assert_eq!(dump.heat_j,0.0);
    }
    #[test]
    fn shared_heat_gates_weapons_and_cooling_restores_them() {
        let mut thermal=Thermal::default();
        thermal.add_waste_heat(BEAM_HEAT_LIMIT_J.value);
        assert!(!thermal.can_fire());assert_eq!(thermal.thrust_factor(),0.0);
        thermal.dumping=true;
        thermal.advance(6000.0,false);
        assert!(!thermal.can_fire());
        thermal.dumping=false;
        assert!(thermal.can_fire());assert_eq!(thermal.thrust_factor(),1.0);
        let before=thermal.heat_j;thermal.fire();
        assert!(thermal.heat_j>before);
        assert!(thermal.balance_error().abs()<1e5);
    }
    #[test]
    fn heat_integration_is_independent_of_time_partition() {
        let mut whole=Thermal::default();let mut stepped=whole;
        let power=Thermal::drive_power(0.8);
        whole.advance_with_drive(3600.0,false,1.0,1.0,power);
        for i in 1..=7200 {stepped.advance_with_drive(i as f64*0.5,false,1.0,1.0,power);}
        assert!((whole.heat_j-stepped.heat_j).abs()/SHIP_HEAT_LIMIT_J<1e-10);
        assert!((whole.radiated_j-stepped.radiated_j).abs()/SHIP_HEAT_LIMIT_J<1e-10);
    }

    #[test]
    fn a_frigate_shot_is_one_percent_and_the_gate_matches_the_heat() {
        let mut thermal=Thermal::default();
        assert!(thermal.can_fire());
        thermal.fire();
        assert!((thermal.heat_fraction()-BEAM_SHOT_HEAT_FRACTION).abs()<1e-12);
        assert!(thermal.balance_error().abs()<1.0);
        for _ in 0..3 {
            thermal.capacitor_j=thermal.capacitor_capacity();
            assert!(thermal.can_fire());
            thermal.fire();
        }
        assert!((thermal.heat_fraction()-4.0*BEAM_SHOT_HEAT_FRACTION).abs()<1e-9);
        assert!(thermal.can_fire(),"four shots stay inside the beam limit");
        let waste=Thermal::beam_waste(SHIP_BEAM_ENERGY_J.value);
        let mut gate=Thermal::default();
        let limit=BEAM_HEAT_LIMIT_J.value*gate.capacity_scale;
        gate.heat_j=limit-waste;
        assert!(gate.can_fire_energy(SHIP_BEAM_ENERGY_J.value),"room for one scaled shot");
        // One joule is smaller than the unit in the last place of this tank, so the overshoot is a tiny representable margin.
        let margin=limit*1e-9;
        gate.heat_j+=margin;
        assert!(margin>1.0 && !gate.can_fire_energy(SHIP_BEAM_ENERGY_J.value),"the last shot must not overshoot");
        assert!((screen_heat_multiplier()*SCREEN_CAPACITY_J.value/SHIP_HEAT_LIMIT_J-SCREEN_HEAT_TANK_FRACTION).abs()<1e-12);
        let mut battleship=Thermal {capacity_scale:8.0,capacitor_j:BEAM_CAPACITOR_J.value*8.0*2.0,capacitor_multiplier:2.0,..Default::default()};
        let spinal=10.0*SHIP_BEAM_ENERGY_J.value*4.0;
        assert!(battleship.can_fire_energy(spinal));
        battleship.fire_energy(spinal);
        assert!((battleship.heat_fraction()-0.05).abs()<1e-9,"a spinal shot is the same waste scale");
        assert!(battleship.balance_error().abs()<1.0);
        assert_eq!(SHIP_BEAM_RECHARGE_S.value,5.0);
    }
    #[test]
    fn microscopic_absorption_never_stalls_cooling() {
        for energy in [f64::MIN_POSITIVE,1e-100,1e-30,1e-12,1.0] {
            let mut thermal=Thermal::default();thermal.absorb(energy);thermal.advance(2.0,false);
            assert_eq!(thermal.last_t,2.0);assert!(thermal.heat_j.is_finite());
        }
    }
}
