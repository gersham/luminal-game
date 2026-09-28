//! Best-effort continuous flight through user-drawn control points.
use crate::kinematics::{State,Vec2};

#[derive(Clone,Debug)]
pub struct FlightRoute {
    pub points:Vec<Vec2>,
    pub progress:f64,
}

impl FlightRoute {
    pub fn new(start:Vec2,point:Vec2)->Self {Self {points:vec![start,point],progress:0.0}}
    pub fn sample(&self,progress:f64)->Vec2 {
        let end=self.points.len()-1;
        let u=progress.clamp(0.0,end as f64);
        let i=(u.floor() as usize).min(end-1);
        let t=u-i as f64;
        let p0=self.points[i.saturating_sub(1)];
        let p1=self.points[i];let p2=self.points[i+1];let p3=self.points[(i+2).min(end)];
        (p1*2.0+(p2-p0)*t+(p0*2.0-p1*5.0+p2*4.0-p3)*(t*t)
            +(-p0+p1*3.0-p2*3.0+p3)*(t*t*t))*0.5
    }

    /// Thrust, monotonic curve progress, and whether the last point was passed.
    pub fn guide(&self,state:State,accel:f64,gravity:Vec2)->(Vec2,f64,bool) {
        let end=(self.points.len()-1) as f64;
        let mut progress=self.progress;
        let mut nearest=f64::INFINITY;
        let stop=(self.progress.floor()+2.0).min(end);
        let count=((stop-self.progress)*32.0).ceil().max(1.0) as usize;
        let mut previous=self.sample(self.progress);
        for i in 1..=count {
            let u=self.progress+(stop-self.progress)*i as f64/count as f64;
            let current=self.sample(u);let delta=current-previous;
            let fraction=((state.pos-previous).dot(delta)/delta.dot(delta).max(1e-12)).clamp(0.0,1.0);
            let distance=(state.pos-(previous+delta*fraction)).length();
            if distance<nearest {
                nearest=distance;
                progress=self.progress+(stop-self.progress)*(i as f64-1.0+fraction)/count as f64;
            }
            previous=current;
        }
        let last=*self.points.last().unwrap();
        let tangent=(last-self.points[self.points.len()-2]).normalized();
        if progress>=end-0.02 && (state.pos-last).dot(tangent)>=0.0 {
            return (Vec2::ZERO,end,true);
        }
        if accel<=0.0 {return (Vec2::ZERO,progress,false);}
        let segment=(progress.floor() as usize).min(self.points.len()-2);
        let span=(self.points[segment+1]-self.points[segment]).length().max(1.0);
        let speed=(accel*span).sqrt().max(1.0);
        let lookahead=(state.vel.length().powi(2)/(2.0*accel)).max(span*0.1).max(10.0);
        let mut remaining=lookahead;
        let mut aim=self.sample(progress);
        let mut u=progress;
        while u<end && remaining>0.0 {
            let next=(u+0.05).min(end);let point=self.sample(next);
            let delta=point-aim;let length=delta.length();
            if length>=remaining {aim=aim+delta.normalized()*remaining;remaining=0.0;}
            else {aim=point;remaining-=length;}
            u=next;
        }
        if remaining>0.0 {aim=aim+tangent*remaining;}
        let desired_velocity=(aim-state.pos).normalized()*speed;
        let response=(lookahead/speed).clamp(5.0,300.0);
        let thrust=(desired_velocity-state.vel)*(1.0/response)-gravity;
        (if thrust.length()>accel {thrust.normalized()*accel} else {thrust},progress,false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn curve_passes_through_control_points() {
        let route=FlightRoute {points:vec![Vec2::ZERO,Vec2::new(1000.0,500.0),Vec2::new(2000.0,0.0)],progress:0.0};
        for (i,p) in route.points.iter().enumerate() {assert!((route.sample(i as f64)-*p).length()<1e-9);}
        let (thrust,_,done)=route.guide(State {pos:Vec2::ZERO,vel:Vec2::ZERO},1.0,Vec2::ZERO);
        assert!(thrust.x>0.0 && thrust.y>0.0 && thrust.length()<=1.0+1e-9 && !done);
    }
    #[test] fn flies_through_curve_without_stopping_and_coasts_at_end() {
        let mut route=FlightRoute {points:vec![Vec2::ZERO,Vec2::new(10_000.0,2000.0),Vec2::new(20_000.0,0.0)],progress:0.0};
        let mut state=State {pos:Vec2::ZERO,vel:Vec2::ZERO};
        let mut passed_middle=false;
        for _ in 0..2000 {
            let (thrust,progress,done)=route.guide(state,1.0,Vec2::ZERO);
            assert!(progress>=route.progress && thrust.length()<=1.0+1e-9);
            route.progress=progress;
            if progress>=1.0 && !passed_middle {assert!(state.vel.length()>10.0);passed_middle=true;}
            if done {assert!(passed_middle && state.vel.length()>10.0);assert_eq!(thrust,Vec2::ZERO);return;}
            state=crate::kinematics::advance(state,thrust,1.0);
        }
        panic!("route did not complete");
    }
}
