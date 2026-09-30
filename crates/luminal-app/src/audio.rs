//! Quiet, bounded, wall-clock audio. Only the player's received view drives cues.
use super::{CombatLogKey, CombatKind};
use luminal_core::{session::{View,BodyId}, mind::ContactId, sensors::DetectionLevel, world::BodyKind};
use rodio::{OutputStream,OutputStreamBuilder,Sink,Decoder,Source,buffer::SamplesBuffer};
use std::{collections::{BTreeMap,BTreeSet},io::Cursor,time::Instant};

#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord)]
pub enum Cue {Click,Ping,Launch,Beam,Spinal,Impact,Explosion,Contact,Alert}
const ASSETS:[(Cue,&[u8]);9]=[
    (Cue::Click,include_bytes!("../../../assets/audio/click.wav")),
    (Cue::Contact,include_bytes!("../../../assets/audio/contact.wav")),
    (Cue::Ping,include_bytes!("../../../assets/audio/ping.wav")),
    (Cue::Launch,include_bytes!("../../../assets/audio/launch.wav")),
    (Cue::Beam,include_bytes!("../../../assets/audio/beam.wav")),
    (Cue::Spinal,include_bytes!("../../../assets/audio/spinal.wav")),
    (Cue::Impact,include_bytes!("../../../assets/audio/impact.wav")),
    (Cue::Explosion,include_bytes!("../../../assets/audio/explosion.wav")),
    (Cue::Alert,include_bytes!("../../../assets/audio/alert.wav")),
];
fn cooldown(cue:Cue)->f64 {match cue {Cue::Click=>0.08,Cue::Beam=>0.5,Cue::Alert=>4.0,Cue::Contact|Cue::Ping=>2.0,_=>0.7}}
/// A lone track still announces. Further tracks in an established picture, and a quiet plot, do not.
pub(super) fn contact_tone(known_tracks:usize,quiet:bool)->bool {!quiet && known_tracks<3}
/// Wall-clock silence before a dropped track, or a fresh ping, may sound again.
const CONTACT_MEMORY_S:f64=15.0;
const CONTACT_LATCH_S:f64=8.0;
const PING_GAP_S:f64=8.0;
pub(super) fn contact_latched(since_tone_s:f64)->bool {since_tone_s<CONTACT_LATCH_S}
pub(super) fn ping_episode(since_fresh_s:f64)->bool {since_fresh_s>=PING_GAP_S}
pub struct Audio {
    stream:Option<OutputStream>, samples:BTreeMap<Cue,SamplesBuffer>, voices:Vec<Sink>,
    played:BTreeMap<Cue,Instant>, last:Option<Instant>,
    pub volume:f32, pub music_volume:f32, pub muted:bool, music:Option<Sink>,
    initialized:bool, combat:BTreeSet<CombatLogKey>, bodies:BTreeSet<BodyId>,
    contacts:BTreeMap<ContactId,DetectionLevel>, contact_seen:BTreeMap<ContactId,Instant>,
    contact_latch:Option<Instant>, pings:BTreeSet<(u64,u64,u64)>, last_fresh_ping:Option<Instant>,
    resolved_announced:BTreeSet<ContactId>, ship_contacts:usize,
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
            initialized:false,combat:BTreeSet::new(),bodies:BTreeSet::new(),contacts:BTreeMap::new(),contact_seen:BTreeMap::new(),
            contact_latch:None,pings:BTreeSet::new(),last_fresh_ping:None,resolved_announced:BTreeSet::new(),ship_contacts:0}
    }
}
impl Audio {
    fn contact_known(&self,id:ContactId)->bool {self.contacts.contains_key(&id) || self.contact_seen.contains_key(&id)}
    fn contact_cue(&mut self,id:ContactId,level:DetectionLevel)->bool {
        let first_resolution=level>=DetectionLevel::Resolved && self.resolved_announced.insert(id);
        first_resolution || (!self.contact_known(id) && !self.resolved_announced.contains(&id))
    }
    fn contact_latched_now(&self,now:Instant)->bool {
        self.contact_latch.is_some_and(|at|contact_latched(now.duration_since(at).as_secs_f64()))
    }
    pub fn play(&mut self,cue:Cue) {
        let now=Instant::now();
        if self.muted || self.volume<=0.0 || self.played.get(&cue).is_some_and(|at|now.duration_since(*at).as_secs_f64()<cooldown(cue))
            || (!matches!(cue,Cue::Contact|Cue::Alert) && self.last.is_some_and(|at|now.duration_since(at).as_secs_f64()<0.075)) {return;}
        let Some(stream)=&self.stream else {return;};
        self.voices.retain(|s|!s.empty());
        // Never build an audio backlog at 1000x. Important alerts replace a voice.
        if self.voices.len()>=4 {if matches!(cue,Cue::Contact|Cue::Alert) {self.voices.remove(0).stop();} else {return;}}
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
    pub fn observe(&mut self,view:&View,own:Option<BodyId>,quiet:bool) {
        let key=|e:&luminal_core::world::CombatEvent|(e.received_at.to_bits(),e.emitted_at.to_bits(),e.kind as u8,e.own_body.map(|b|b.0),e.contact.map(|c|c.0));
        let mut cues=BTreeSet::new();
        for event in &view.combat {
            if self.combat.contains(&key(event)) {continue;}
            match event.kind {
                CombatKind::Impact=>{cues.insert(if event.own_body==own && own.is_some() && event.impact_strength>=0.65 {Cue::Alert} else {Cue::Impact});},
                CombatKind::NuclearBurst|CombatKind::Destroyed=>{cues.insert(Cue::Explosion);},
                CombatKind::SpinalPulse=>if !quiet {cues.insert(Cue::Spinal);},
                CombatKind::BeamPulse|CombatKind::PointDefence=>if !quiet {cues.insert(Cue::Beam);},
                _=>{}
            }
        }
        self.combat=view.combat.iter().map(key).collect();
        for b in &view.bodies {if b.kind==BodyKind::Missile && !self.bodies.contains(&b.id) {cues.insert(Cue::Launch);}}
        self.bodies=view.bodies.iter().map(|b|b.id).collect();
        let now=Instant::now();
        let known=self.ship_contacts;
        self.contact_seen.retain(|_,seen|now.duration_since(*seen).as_secs_f64()<CONTACT_MEMORY_S);
        let mut tracks=0usize;
        for c in &view.contacts {
            if c.resolved_missile {
                if !self.contact_known(c.id) {cues.insert(Cue::Alert);}
                self.contact_seen.insert(c.id,now);
                continue;
            }
            tracks+=1;
            let fresh=self.contact_cue(c.id,c.detection) && contact_tone(known,quiet);
            if fresh && self.initialized && !self.contact_latched_now(now) {
                cues.insert(Cue::Contact);
                self.contact_latch=Some(now);
            }
            self.contact_seen.insert(c.id,now);
        }
        self.ship_contacts=tracks;
        self.contacts=view.contacts.iter().map(|c|(c.id,c.detection)).collect();
        let mut fresh_ping=false;
        for p in &view.pings {
            if self.pings.insert((p.origin.x.to_bits(),p.origin.y.to_bits(),p.t_emit.to_bits())) {fresh_ping=true;}
        }
        self.pings.retain(|key|view.pings.iter().any(|p|(p.origin.x.to_bits(),p.origin.y.to_bits(),p.t_emit.to_bits())==*key));
        if fresh_ping {
            let gap=self.last_fresh_ping.map(|at|now.duration_since(at).as_secs_f64()).unwrap_or(1e9);
            self.last_fresh_ping=Some(now);
            if self.initialized && ping_episode(gap) {cues.insert(Cue::Ping);}
        }
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
        assert!(Cue::Contact>Cue::Explosion && Cue::Alert>Cue::Contact);
        assert!(cooldown(Cue::Beam)>cooldown(Cue::Click));assert!(cooldown(Cue::Alert)>=4.0);
        assert!(contact_tone(0,false) && contact_tone(2,false));
        assert!(!contact_tone(3,false) && !contact_tone(0,true));
        assert!(contact_latched(0.0) && contact_latched(7.9));
        assert!(!contact_latched(8.0));
        // 1000× turns a 60s auto-ping into a 0.06s wall gap. That stays one episode.
        // A 1× ping, a minute of wall silence later, sounds again.
        assert!(!ping_episode(0.05) && !ping_episode(0.06) && !ping_episode(7.9));
        assert!(ping_episode(8.0) && ping_episode(60.0));
    }
    #[test] fn a_blinked_contact_stays_quiet_until_the_memory_expires() {
        let mut audio=Audio::default();
        let id=ContactId(4);
        let now=Instant::now();
        assert!(audio.contact_cue(id,DetectionLevel::Bearing));
        audio.contacts.insert(id,DetectionLevel::Bearing);
        audio.contact_seen.insert(id,now);
        audio.contacts.clear();
        assert!(!audio.contact_cue(id,DetectionLevel::Bearing),"a dropped bearing inside the window is the same track");
        audio.contact_seen.insert(id,now-std::time::Duration::from_secs_f64(CONTACT_MEMORY_S+1.0));
        audio.contact_seen.retain(|_,seen|now.duration_since(*seen).as_secs_f64()<CONTACT_MEMORY_S);
        assert!(audio.contact_cue(id,DetectionLevel::Bearing));
    }
    #[test] fn ambient_loop_is_finite_quiet_and_stereo() {
        let s=Decoder::try_from(Cursor::new(include_bytes!("../../../assets/audio/ambient.wav").as_slice())).unwrap();
        assert_eq!(s.channels(),2);assert_eq!(s.sample_rate(),24000);
        let samples:Vec<_>=s.collect();assert_eq!(samples.len(),56*24000*2);
        assert!(samples.iter().all(|v|v.is_finite() && v.abs()<0.26));
    }
}
