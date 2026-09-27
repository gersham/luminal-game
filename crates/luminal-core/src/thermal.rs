//! Conservative prototype power and field model. All tuning is in params.
use crate::params::*;

#[derive(Clone, Copy, Debug)]
pub struct Thermal {
    pub capacitor_j: f64,
    pub heat_j: f64,
    pub field: f64,
    pub generated_j: f64,
    pub radiated_j: f64,
    pub captured_j: f64,
    pub emitted_j: f64,
    pub last_t: f64,
}

impl Default for Thermal {
    fn default() -> Self {
        Self { capacitor_j: BEAM_CAPACITOR_J.value, heat_j: 0.0, field: 0.0,
            generated_j: 0.0, radiated_j: 0.0, captured_j: 0.0, emitted_j: 0.0, last_t: 0.0 }
    }
}

impl Thermal {
    pub fn temperature(screen_j: f64) -> f64 { screen_j / SCREEN_HEAT_CAPACITY.value }
    pub fn screen_emissivity(screen_j:f64)->f64 {
        1.0+(SCREEN_MAX_EMISSIVITY_FACTOR.value-1.0)*(screen_j/SCREEN_CAPACITY_J.value).clamp(0.0,1.0)
    }
    pub fn screen_radiation(screen_j:f64)->f64 {
        SCREEN_RADIATION_COEFF.value*Self::screen_emissivity(screen_j)*Self::temperature(screen_j).powi(4)
    }
    pub fn emission(&self, screen_j: f64) -> f64 {
        Self::screen_radiation(screen_j)
            + self.heat_j / HULL_COOLING_S.value
    }
    pub fn advance(&mut self, t: f64, up: bool, screen_j: &mut f64) {
        self.advance_scaled(t,up,screen_j,1.0,1.0);
    }
    pub fn advance_scaled(&mut self,t:f64,up:bool,screen_j:&mut f64,power:f64,screen:f64) {
        let mut remaining = (t - self.last_t).max(0.0);
        while remaining > 1e-8 {
            let dt = remaining.min(1.0);
            remaining -= dt;
            let recharge = (BEAM_CAPACITOR_J.value - self.capacitor_j).min(REACTOR_W.value * power * dt).max(0.0);
            self.capacitor_j += recharge;
            self.generated_j += recharge;
            if up {
                self.field = (self.field + screen * dt / SCREEN_BUILD_TIME_S.value).min(1.0);
            }
            // Emissivity changes with stored heat. Limit each cooling substep to
            // about 1% of the reservoir and account for every radiated joule.
            let mut cooling_dt=dt;
            while cooling_dt>1e-10 && *screen_j>0.0 {
                let power=Self::screen_radiation(*screen_j);
                // Tiny grazing hits can underflow the physical radiated power.
                // A fabricated power floor made their substep smaller than the
                // clock's precision, hanging the UI forever. No power: no cooling.
                if power<=0.0 {break;}
                let step=cooling_dt.min(0.01**screen_j/power);
                if cooling_dt-step==cooling_dt {break;}
                // Algebraically equivalent cooling solution, without T^-3
                // overflow for a nearly cold screen. Preserve unresolvable heat.
                let after = *screen_j/(1.0+3.0*(power / *screen_j)*step).cbrt();
                self.radiated_j += *screen_j - after;
                *screen_j = after;
                cooling_dt-=step;
            }
            let loss = self.heat_j * (1.0 - (-dt / HULL_COOLING_S.value).exp());
            self.heat_j -= loss;
            self.radiated_j += loss;
            if !up {
                self.field = (self.field - dt / SCREEN_COOL_SHUTDOWN_S.value).max(*screen_j / SCREEN_CAPACITY_J.value).clamp(0.0, 1.0);
            }
        }
        self.last_t = self.last_t.max(t);
    }
    pub fn can_fire(&self) -> bool {
        self.capacitor_j >= SHIP_BEAM_ENERGY_J.value / BEAM_EFFICIENCY.value
            && self.heat_j + SHIP_BEAM_ENERGY_J.value * (1.0 / BEAM_EFFICIENCY.value - 1.0) <= BEAM_HEAT_LIMIT_J.value
    }
    pub fn fire(&mut self) {
        let input = SHIP_BEAM_ENERGY_J.value / BEAM_EFFICIENCY.value;
        self.capacitor_j -= input;
        self.heat_j += input - SHIP_BEAM_ENERGY_J.value;
        self.emitted_j += SHIP_BEAM_ENERGY_J.value;
    }
    pub fn balance_error(&self, screen_j: f64) -> f64 {
        BEAM_CAPACITOR_J.value + self.generated_j + self.captured_j
            - self.capacitor_j - self.heat_j - screen_j - self.radiated_j - self.emitted_j
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubled_beam_cadence_is_supported_by_power_and_cooling() {
        let mut thermal=Thermal::default();
        let mut screen=0.0;
        let mut next=0.0;
        let mut shots=0;
        let mut old_cap=BEAM_CAPACITOR_J.value;
        let mut old_heat=0.0;
        let mut old_next=0.0;
        let mut old_shots=0;
        let input=SHIP_BEAM_ENERGY_J.value/BEAM_EFFICIENCY.value;
        let waste=input-SHIP_BEAM_ENERGY_J.value;
        for tick in 0..=3600 {
            let t=tick as f64*0.5;
            thermal.advance(t,false,&mut screen);
            if t>=next && thermal.can_fire() {
                thermal.fire();shots+=1;next=t+SHIP_BEAM_RECHARGE_S.value;
            }
            if tick>0 {
                old_cap=(old_cap+7.5e12*0.5).min(BEAM_CAPACITOR_J.value);
                old_heat*=(-0.5_f64/120.0).exp();
            }
            if t>=old_next && old_cap>=input && old_heat+waste<=BEAM_HEAT_LIMIT_J.value {
                old_cap-=input;old_heat+=waste;old_shots+=1;old_next=t+10.0;
            }
        }
        let ratio=shots as f64/old_shots as f64;
        assert!((1.9..2.1).contains(&ratio),"new {shots}, old {old_shots}");
        assert_eq!(SHIP_BEAM_RECHARGE_S.value,5.0);
    }
    #[test]
    fn microscopic_beam_heat_cannot_stall_cooling() {
        for energy in [f64::MIN_POSITIVE,1e-100,2.1342756116755366e-80,1e-30,1e-12,1.0] {
            let mut thermal=Thermal::default();
            let mut heat=energy;
            thermal.captured_j=energy;
            thermal.advance(2.0,true,&mut heat);
            assert_eq!(thermal.last_t,2.0);
            assert!(heat.is_finite() && heat>=0.0 && heat<=energy);
            assert!(thermal.radiated_j>=0.0);
        }
    }
    #[test]
    fn screens_have_no_ambient_heat_even_while_raised_and_firing() {
        let mut thermal=Thermal::default();
        let mut energy=0.0;
        thermal.advance(3600.0,true,&mut energy);
        assert_eq!(thermal.field,1.0);
        assert_eq!(energy,0.0);
        thermal.fire();
        thermal.advance(7200.0,true,&mut energy);
        assert_eq!(energy,0.0,"weapon waste heat must not enter the screen");
        assert!(thermal.balance_error(energy).abs()<1.0);
        energy=SCREEN_CAPACITY_J.value*0.5;
        thermal.captured_j+=energy;
        let hit_heat=energy;
        thermal.advance(7260.0,true,&mut energy);
        assert!(energy>0.0 && energy<hit_heat,"absorbed damage still cools while raised");
        assert!(thermal.balance_error(energy).abs()<1.0);
    }
    #[test]
    fn hot_screen_reaches_100x_emissivity_and_cools_without_deleting_energy() {
        let capacity=SCREEN_CAPACITY_J.value;
        assert_eq!(Thermal::screen_emissivity(0.0),1.0);
        assert_eq!(Thermal::screen_emissivity(capacity),100.0);
        assert_eq!(Thermal::screen_emissivity(capacity*2.0),100.0);
        assert_eq!(Thermal::screen_emissivity(capacity*0.5),50.5);
        let ordinary=SCREEN_RADIATION_COEFF.value*Thermal::temperature(capacity).powi(4);
        assert!((Thermal::screen_radiation(capacity)/ordinary-100.0).abs()<1e-10);
        let mut thermal=Thermal {field:1.0,captured_j:capacity,..Default::default()};
        let mut energy=capacity;
        thermal.advance(1.0,false,&mut energy);
        assert!(energy>0.0 && energy<capacity);
        assert!(thermal.radiated_j>0.0 && thermal.field>0.0);
        assert!(Thermal::screen_emissivity(energy)<100.0);
        assert!(thermal.balance_error(energy).abs()<capacity*1e-12);
        let previous=energy;
        thermal.advance(60.0,false,&mut energy);
        assert!(energy>0.0 && energy<previous);
        assert!(thermal.balance_error(energy).abs()<capacity*1e-12);
    }

    #[test]
    fn energy_survives_firing_heating_and_hot_shutdown() {
        let mut s = Thermal::default();
        let mut e = 0.0;
        s.advance(60.0, true, &mut e);
        assert_eq!(s.field, 1.0);
        s.fire();
        e += SCREEN_CAPACITY_J.value * 0.8;
        s.captured_j += SCREEN_CAPACITY_J.value * 0.8;
        s.advance(180.0, false, &mut e);
        assert!(s.field > 0.0 && e > 0.0);
        assert!(s.balance_error(e).abs() < 100.0);
    }
}
