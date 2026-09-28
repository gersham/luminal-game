//! Quiet, bounded, wall-clock audio. Only the player's received view drives cues.
use super::{CombatLogKey, CombatKind};
use luminal_core::{session::{View,BodyId}, mind::ContactId, sensors::DetectionLevel, world::BodyKind};
use rodio::{OutputStream,OutputStreamBuilder,Sink,Decoder,Source,buffer::SamplesBuffer};
use std::{collections::{BTreeMap,BTreeSet},io::Cursor,time::Instant};

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]
pub enum Cue {Click,Contact,Ping,Launch,Beam,Impact,Explosion,Alert}
const ASSETS:[(Cue,&[u8]);8]=[
    (Cue::Click,include_bytes!("../../../assets/audio/click.wav")),
    (Cue::Contact,include_bytes!("../../../assets/audio/contact.wav")),
    (Cue::Ping,include_bytes!("../../../assets/audio/ping.wav")),
    (Cue::Launch,include_bytes!("../../../assets/audio/launch.wav")),
    (Cue::Beam,include_bytes!("../../../assets/audio/beam.wav")),
    (Cue::Impact,include_bytes!("../../../assets/audio/impact.wav")),
    (Cue::Explosion,include_bytes!("../../../assets/audio/explosion.wav")),
    (Cue::Alert,include_bytes!("../../../assets/audio/alert.wav")),
];
fn cooldown(cue:Cue)->f64 {match cue {Cue::Click=>0.08,Cue::Beam=>0.5,Cue::Alert=>4.0,Cue::Contact|Cue::Ping=>2.0,_=>0.7}}
pub struct Audio {
    stream:Option<OutputStream>, samples:BTreeMap<Cue,SamplesBuffer>, voices:Vec<Sink>,
    played:BTreeMap<Cue,Instant>, last:Option<Instant>,
    pub volume:f32, pub music_volume:f32, pub muted:bool, music:Option<Sink>,
    initialized:bool, combat:BTreeSet<CombatLogKey>, bodies:BTreeSet<BodyId>,
    contacts:BTreeMap<ContactId,DetectionLevel>, pings:BTreeSet<(u64,u64,u64)>,
    resolved_announced:BTreeSet<ContactId>,
}
impl Default for Audio {
    fn default()->Self {
        let stream=if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {None}
            else {match OutputStreamBuilder::open_default_stream() {Ok(mut s)=>{s.log_on_drop(false);Some(s)},Err(e)=>{eprintln!("Audio unavailable (continuing silently): {e}");None}}};
        let samples=ASSETS.into_iter().filter_map(|(cue,bytes)|{
            let source=Decoder::try_from(Cursor::new(bytes)).ok()?;
            Some((cue,SamplesBuffer::new(source.channels(),source.sample_rate(),source.collect::<Vec<f32>>())))
        }).collect();
        let music=stream.as_ref().and_then(|stream| {
            let decoder=Decoder::try_from(Cursor::new(include_bytes!("../../../assets/audio/ambient.wav").as_slice())).ok()?;
            let sink=Sink::connect_new(stream.mixer());sink.set_volume(0.25);
            sink.append(decoder.buffered().repeat_infinite().fade_in(std::time::Duration::from_secs(3)));
            Some(sink)
        });
        Self {stream,samples,voices:Vec::new(),played:BTreeMap::new(),last:None,volume:0.35,music_volume:0.25,muted:false,music,
            initialized:false,combat:BTreeSet::new(),bodies:BTreeSet::new(),contacts:BTreeMap::new(),pings:BTreeSet::new(),resolved_announced:BTreeSet::new()}
    }
}
impl Audio {
    fn contact_cue(&mut self,id:ContactId,level:DetectionLevel)->bool {
        let first_resolution=level>=DetectionLevel::Resolved && self.resolved_announced.insert(id);
        first_resolution || (!self.contacts.contains_key(&id) && !self.resolved_announced.contains(&id))
    }
    pub fn play(&mut self,cue:Cue) {
        let now=Instant::now();
        if self.muted || self.volume<=0.0 || self.played.get(&cue).is_some_and(|at|now.duration_since(*at).as_secs_f64()<cooldown(cue))
            || self.last.is_some_and(|at|now.duration_since(at).as_secs_f64()<0.075) {return;}
        let Some(stream)=&self.stream else {return;};
        self.voices.retain(|s|!s.empty());
        // Never build an audio backlog at 1000x. Important alerts replace a voice.
        if self.voices.len()>=4 {if cue==Cue::Alert {self.voices.remove(0).stop();} else {return;}}
        let Some(sample)=self.samples.get(&cue) else {return;};
        let sink=Sink::connect_new(stream.mixer());
        sink.set_volume(self.volume*match cue {Cue::Click=>0.55,Cue::Beam=>0.45,Cue::Ping=>0.6,_=>1.0});
        sink.append(sample.clone());self.voices.push(sink);
        self.played.insert(cue,now);self.last=Some(now);
    }
    pub fn settings_changed(&mut self) {
        for sink in self.voices.drain(..) {sink.stop();}
        if let Some(music)=&self.music {music.set_volume(if self.muted {0.0} else {self.music_volume});}
    }
    pub fn observe(&mut self,view:&View,own:Option<BodyId>) {
        let key=|e:&luminal_core::world::CombatEvent|(e.received_at.to_bits(),e.emitted_at.to_bits(),e.kind as u8,e.own_body.map(|b|b.0),e.contact.map(|c|c.0));
        let mut cues=BTreeSet::new();
        for event in &view.combat {
            if self.combat.contains(&key(event)) {continue;}
            match event.kind {
                CombatKind::Impact=>{cues.insert(if event.own_body==own && own.is_some() && event.impact_strength>=0.65 {Cue::Alert} else {Cue::Impact});},
                CombatKind::NuclearBurst|CombatKind::Destroyed=>{cues.insert(Cue::Explosion);},
                CombatKind::BeamPulse|CombatKind::PointDefence=>{cues.insert(Cue::Beam);},
                _=>{}
            }
        }
        self.combat=view.combat.iter().map(key).collect();
        for b in &view.bodies {if b.kind==BodyKind::Missile && !self.bodies.contains(&b.id) {cues.insert(Cue::Launch);}}
        self.bodies=view.bodies.iter().map(|b|b.id).collect();
        for c in &view.contacts {
            if !c.resolved_missile && self.contact_cue(c.id,c.detection) {cues.insert(Cue::Contact);}
            if c.resolved_missile && !self.contacts.contains_key(&c.id) {cues.insert(Cue::Alert);}
        }
        self.contacts=view.contacts.iter().map(|c|(c.id,c.detection)).collect();
        for p in &view.pings {if !self.pings.contains(&(p.origin.x.to_bits(),p.origin.y.to_bits(),p.t_emit.to_bits())) {cues.insert(Cue::Ping);}}
        self.pings=view.pings.iter().map(|p|(p.origin.x.to_bits(),p.origin.y.to_bits(),p.t_emit.to_bits())).collect();
        // Seed history without replaying startup/pre-run events. Highest urgency wins a frame.
        if self.initialized {for cue in cues.into_iter().rev() {self.play(cue);}}
        self.initialized=true;
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn refreshes_do_not_reannounce_a_resolved_contact() {
        let mut audio=Audio::default();let id=ContactId(1);
        assert!(audio.contact_cue(id,DetectionLevel::Bearing));
        audio.contacts.insert(id,DetectionLevel::Bearing);
        assert!(audio.contact_cue(id,DetectionLevel::Identity));
        for level in [DetectionLevel::Approximate,DetectionLevel::Resolved,DetectionLevel::Identity] {
            assert!(!audio.contact_cue(id,level));audio.contacts.insert(id,level);
        }
        audio.contacts.clear();
        assert!(!audio.contact_cue(id,DetectionLevel::Identity));
        assert!(audio.contact_cue(ContactId(2),DetectionLevel::Identity));
    }
    #[test] fn all_assets_decode_and_have_safe_peaks() {
        for (_,bytes) in ASSETS {let s=Decoder::try_from(Cursor::new(bytes)).unwrap();let samples:Vec<_>=s.collect();
            assert!(!samples.is_empty());let peak=samples.iter().map(|v|v.abs()).fold(0.0_f32,f32::max);
            assert!(peak>0.001 && peak<0.26,"asset peak {peak}");}
    }
    #[test] fn combat_audio_is_slower_than_ui_and_alerts_are_throttled() {
        assert!(cooldown(Cue::Beam)>cooldown(Cue::Click));assert!(cooldown(Cue::Alert)>=4.0);
    }
    #[test] fn ambient_loop_is_finite_quiet_and_stereo() {
        let s=Decoder::try_from(Cursor::new(include_bytes!("../../../assets/audio/ambient.wav").as_slice())).unwrap();
        assert_eq!(s.channels(),2);assert_eq!(s.sample_rate(),24000);
        let samples:Vec<_>=s.collect();assert_eq!(samples.len(),56*24000*2);
        assert!(samples.iter().all(|v|v.is_finite() && v.abs()<0.26));
    }
}
