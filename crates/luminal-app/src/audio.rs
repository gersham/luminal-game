//! Quiet, bounded, wall-clock audio. Only the player's received view drives cues.
use super::{CombatLogKey, CombatKind};
use luminal_core::{session::{View,BodyId}, mind::ContactId, sensors::DetectionLevel, world::BodyKind};
use rodio::{OutputStream,OutputStreamBuilder,Sink,Decoder,Source,buffer::SamplesBuffer};
use std::{collections::{BTreeMap,BTreeSet},io::Cursor,time::Instant};

/// Ordered by urgency: when several fall in one frame the later ones are played first.
#[derive(Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Debug)]
pub enum Cue {Click,Ping,PointDefence,Launch,Beam,JumpSpool,Jump,Spinal,Impact,Explosion,Contact,Alert}
const ASSETS:[(Cue,&[u8]);12]=[
    (Cue::Click,include_bytes!("../../../assets/audio/click.wav")),
    (Cue::Contact,include_bytes!("../../../assets/audio/contact.wav")),
    (Cue::Ping,include_bytes!("../../../assets/audio/ping.wav")),
    (Cue::PointDefence,include_bytes!("../../../assets/audio/pd.wav")),
    (Cue::Launch,include_bytes!("../../../assets/audio/launch.wav")),
    (Cue::Beam,include_bytes!("../../../assets/audio/beam.wav")),
    (Cue::JumpSpool,include_bytes!("../../../assets/audio/jump-spool.wav")),
    (Cue::Jump,include_bytes!("../../../assets/audio/jump.wav")),
    (Cue::Spinal,include_bytes!("../../../assets/audio/spinal.wav")),
    (Cue::Impact,include_bytes!("../../../assets/audio/impact.wav")),
    (Cue::Explosion,include_bytes!("../../../assets/audio/explosion.wav")),
    (Cue::Alert,include_bytes!("../../../assets/audio/alert.wav")),
];
const DRIVE:&[u8]=include_bytes!("../../../assets/audio/drive.wav");
fn cooldown(cue:Cue)->f64 {match cue {Cue::Click=>0.08,Cue::PointDefence=>0.3,Cue::Beam=>0.5,Cue::Alert=>4.0,Cue::Contact|Cue::Ping=>2.0,Cue::JumpSpool=>3.0,_=>0.7}}
/// Level of each cue against the effects slider. Routine and frequent sounds sit under rare ones.
fn gain(cue:Cue)->f32 {match cue {Cue::Click=>0.5,Cue::PointDefence=>0.4,Cue::Beam=>0.45,Cue::Ping=>0.7,Cue::JumpSpool=>0.7,
    Cue::Jump=>0.9,_=>1.0}}
/// How far a cue's pitch may wander between plays. Signals keep their pitch; physical sounds never repeat exactly.
fn pitch_spread(cue:Cue)->f32 {match cue {Cue::Contact|Cue::Alert|Cue::Click=>0.0,Cue::Ping|Cue::JumpSpool=>0.01,_=>0.045}}
/// Most a sound is moved toward one speaker. The far side never goes silent.
const PAN_LIMIT:f32=0.7;
/// Six voices, the drive and the music. Their peaks would all have to coincide at a full slider to reach full scale.
const VOICES:usize=6;
/// Contact and alert. They are never held back, and never hold back the sound they announce.
fn signal(cue:Cue)->bool {matches!(cue,Cue::Contact|Cue::Alert)}
/// Routine sounds leave a gap after one another, so a busy frame is one sound and not a smear.
pub(super) fn crowded(cue:Cue,since_routine_s:Option<f64>)->bool {!signal(cue) && since_routine_s.is_some_and(|gap|gap<0.075)}
/// Left and right gains for a place between -1 and 1. The centre is unity on both sides.
pub(super) fn balance(pan:f32)->(f32,f32) {
    let pan=if pan.is_finite() {pan.clamp(-1.0,1.0)*PAN_LIMIT} else {0.0};
    ((1.0-pan).min(1.0),(1.0+pan).min(1.0))
}
/// Drive loop level and pitch for a thrust fraction. Silent at rest, and never above the effects.
pub(super) fn drive_voice(thrust_fraction:f32)->(f32,f32) {
    let f=if thrust_fraction.is_finite() {thrust_fraction.clamp(0.0,1.0)} else {0.0};
    (0.6*f.sqrt(),0.88+0.3*f)
}
/// A lone track still announces. Further tracks in an established picture, and a quiet plot, do not.
pub(super) fn contact_tone(known_tracks:usize,quiet:bool)->bool {!quiet && known_tracks<3}
/// Wall-clock silence before a dropped track, or a fresh ping, may sound again.
const CONTACT_MEMORY_S:f64=15.0;
const CONTACT_LATCH_S:f64=8.0;
const PING_GAP_S:f64=8.0;
pub(super) fn contact_latched(since_tone_s:f64)->bool {since_tone_s<CONTACT_LATCH_S}
pub(super) fn ping_episode(since_fresh_s:f64)->bool {since_fresh_s>=PING_GAP_S}
struct Clip {channels:u16,rate:u32,data:Vec<f32>}
impl Clip {
    fn decode(bytes:&'static [u8])->Option<Self> {
        let source=Decoder::try_from(Cursor::new(bytes)).ok()?;
        Some(Self {channels:source.channels(),rate:source.sample_rate(),data:source.collect()})
    }
    /// A stereo copy placed at `pan`. A mono clip is spread to both sides first.
    fn placed(&self,pan:f32)->SamplesBuffer {
        let (left,right)=balance(pan);
        let data:Vec<f32>=if self.channels==2 {
            self.data.as_chunks::<2>().0.iter().flat_map(|frame|[frame[0]*left,frame[1]*right]).collect()
        } else {self.data.iter().flat_map(|v|[v*left,v*right]).collect()};
        SamplesBuffer::new(2,self.rate,data)
    }
}
pub struct Audio {
    stream:Option<OutputStream>, samples:BTreeMap<Cue,Clip>, voices:Vec<Sink>,
    played:BTreeMap<Cue,Instant>, last:Option<Instant>,
    pub volume:f32, pub music_volume:f32, pub muted:bool, music:Option<Sink>,
    drive:Option<Sink>, drive_level:f32, noise:u64,
    initialized:bool, combat:BTreeSet<CombatLogKey>, jumps:BTreeSet<CombatLogKey>, bodies:BTreeSet<BodyId>,
    contacts:BTreeMap<ContactId,DetectionLevel>, contact_seen:BTreeMap<ContactId,Instant>,
    contact_latch:Option<Instant>, pings:BTreeSet<(u64,u64,u64)>, last_fresh_ping:Option<Instant>,
    resolved_announced:BTreeSet<ContactId>, ship_contacts:usize,
}
impl Default for Audio {
    fn default()->Self {
        let stream=if cfg!(test) || std::env::var_os("LUMINAL_SCREENSHOT").is_some() {None}
            else {match OutputStreamBuilder::open_default_stream() {Ok(mut s)=>{s.log_on_drop(false);Some(s)},Err(e)=>{eprintln!("Audio unavailable (continuing silently): {e}");None}}};
        let samples=ASSETS.into_iter().filter_map(|(cue,bytes)|Some((cue,Clip::decode(bytes)?))).collect();
        let music=stream.as_ref().and_then(|stream| {
            let decoder=Decoder::try_from(Cursor::new(include_bytes!("../../../assets/audio/ambient.wav").as_slice())).ok()?;
            let sink=Sink::connect_new(stream.mixer());sink.set_volume(0.25);
            sink.append(decoder.buffered().repeat_infinite().fade_in(std::time::Duration::from_secs(3)));
            Some(sink)
        });
        // The drive runs all the time at zero level, so thrust fades it in without a start click.
        let drive=stream.as_ref().and_then(|stream| {
            let decoder=Decoder::try_from(Cursor::new(DRIVE)).ok()?;
            let sink=Sink::connect_new(stream.mixer());sink.set_volume(0.0);
            sink.append(decoder.buffered().repeat_infinite());
            Some(sink)
        });
        Self {stream,samples,voices:Vec::new(),played:BTreeMap::new(),last:None,volume:0.35,music_volume:0.25,muted:false,music,
            drive,drive_level:0.0,noise:0x9e37_79b9_7f4a_7c15,
            initialized:false,combat:BTreeSet::new(),jumps:BTreeSet::new(),bodies:BTreeSet::new(),contacts:BTreeMap::new(),contact_seen:BTreeMap::new(),
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
    /// Uniform in 0..1. Cosmetic variation only; nothing in the game reads it.
    fn roll(&mut self)->f32 {
        self.noise^=self.noise<<13;self.noise^=self.noise>>7;self.noise^=self.noise<<17;
        (self.noise>>40) as f32/(1u64<<24) as f32
    }
    /// An interface sound, in the middle.
    pub fn play(&mut self,cue:Cue) {self.play_at(cue,0.0);}
    /// A sound from somewhere on the plot: `pan` is -1 at the left edge of the map, 1 at the right.
    pub fn play_at(&mut self,cue:Cue,pan:f32) {
        let now=Instant::now();
        if self.muted || self.volume<=0.0 || self.played.get(&cue).is_some_and(|at|now.duration_since(*at).as_secs_f64()<cooldown(cue))
            || crowded(cue,self.last.map(|at|now.duration_since(at).as_secs_f64())) {return;}
        if self.stream.is_none() {return;}
        self.voices.retain(|s|!s.empty());
        // Never build an audio backlog at 1000x. Important alerts replace a voice.
        if self.voices.len()>=VOICES {if signal(cue) {self.voices.remove(0).stop();} else {return;}}
        let spread=pitch_spread(cue);
        let (pitch,level)=(1.0+spread*(2.0*self.roll()-1.0),if spread>0.0 {0.86+0.14*self.roll()} else {1.0});
        let (Some(stream),Some(clip))=(&self.stream,self.samples.get(&cue)) else {return;};
        let sink=Sink::connect_new(stream.mixer());
        sink.set_volume(self.volume*gain(cue)*level);
        sink.set_speed(pitch);
        sink.append(clip.placed(pan));self.voices.push(sink);
        self.played.insert(cue,now);
        if !signal(cue) {self.last=Some(now);}
    }
    pub fn settings_changed(&mut self) {
        for sink in self.voices.drain(..) {sink.stop();}
        if let Some(music)=&self.music {music.set_volume(if self.muted {0.0} else {self.music_volume});}
        if self.muted || self.volume<=0.0 {self.drive_level=0.0;if let Some(drive)=&self.drive {drive.set_volume(0.0);}}
    }
    /// Ease the drive loop toward the command ship's thrust. `dt` is wall-clock seconds.
    fn drive_follow(&mut self,thrust_fraction:f32,dt:f32) {
        let (level,pitch)=drive_voice(thrust_fraction);
        let target=if self.muted {0.0} else {level*self.volume};
        self.drive_level+=(target-self.drive_level)*(1.0-(-dt.clamp(0.0,0.25)/0.35).exp());
        if let Some(drive)=&self.drive {drive.set_volume(self.drive_level);drive.set_speed(pitch);}
    }
    /// `place` maps a received position to a pan. It sees only what the view holds.
    pub fn observe(&mut self,view:&View,own:Option<BodyId>,quiet:bool,dt:f32,place:impl Fn(luminal_core::kinematics::Vec2)->f32) {
        let key=|e:&luminal_core::world::CombatEvent|(e.received_at.to_bits(),e.emitted_at.to_bits(),e.kind as u8,e.own_body.map(|b|b.0),e.contact.map(|c|c.0));
        let at=|e:&luminal_core::world::CombatEvent|e.pos.map_or(0.0,&place);
        let mut cues:BTreeMap<Cue,f32>=BTreeMap::new();
        for event in &view.combat {
            if self.combat.contains(&key(event)) {continue;}
            let pan=at(event);
            match event.kind {
                CombatKind::Impact=>{
                    let mine=event.own_body==own && own.is_some();
                    if mine && event.impact_strength>=0.65 {cues.entry(Cue::Alert).or_insert(0.0);}
                    cues.entry(Cue::Impact).or_insert(pan);
                },
                CombatKind::NuclearBurst|CombatKind::Destroyed=>{cues.entry(Cue::Explosion).or_insert(pan);},
                CombatKind::SpinalPulse=>if !quiet {cues.entry(Cue::Spinal).or_insert(pan);},
                CombatKind::BeamPulse=>if !quiet {cues.entry(Cue::Beam).or_insert(pan);},
                CombatKind::PointDefence=>if !quiet {cues.entry(Cue::PointDefence).or_insert(pan);},
                _=>{}
            }
        }
        self.combat=view.combat.iter().map(key).collect();
        for event in &view.jump_events {
            if self.jumps.contains(&key(event)) {continue;}
            match event.kind {
                CombatKind::JumpSpool=>{cues.entry(Cue::JumpSpool).or_insert(at(event));},
                CombatKind::JumpDeparture|CombatKind::JumpArrival=>{cues.entry(Cue::Jump).or_insert(at(event));},
                _=>{}
            }
        }
        self.jumps=view.jump_events.iter().map(key).collect();
        for b in &view.bodies {if b.kind==BodyKind::Missile && !self.bodies.contains(&b.id) {cues.entry(Cue::Launch).or_insert(place(b.pos));}}
        self.bodies=view.bodies.iter().map(|b|b.id).collect();
        let thrust=own.and_then(|id|view.bodies.iter().find(|b|b.id==id)).filter(|_|!view.paused)
            .map_or(0.0,|b|(b.thrust.length()/luminal_core::units::G0/b.ship_class.map_or(120.0,|c|c.max_g())) as f32);
        self.drive_follow(thrust,dt);
        let now=Instant::now();
        let known=self.ship_contacts;
        self.contact_seen.retain(|_,seen|now.duration_since(*seen).as_secs_f64()<CONTACT_MEMORY_S);
        let mut tracks=0usize;
        for c in &view.contacts {
            if c.resolved_missile {
                if !self.contact_known(c.id) {cues.entry(Cue::Alert).or_insert(0.0);}
                self.contact_seen.insert(c.id,now);
                continue;
            }
            tracks+=1;
            let fresh=self.contact_cue(c.id,c.detection) && contact_tone(known,quiet);
            if fresh && self.initialized && !self.contact_latched_now(now) {
                cues.entry(Cue::Contact).or_insert(0.0);
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
            if self.initialized && ping_episode(gap) {cues.entry(Cue::Ping).or_insert(0.0);}
        }
        // Seed history without replaying startup/pre-run events. Highest urgency wins a frame.
        if self.initialized {for (cue,pan) in cues.into_iter().rev() {self.play_at(cue,pan);}}
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
    #[test] fn placed_sounds_lean_without_losing_a_side_and_signals_keep_their_pitch() {
        assert_eq!(balance(0.0),(1.0,1.0));
        let (left,right)=balance(1.0);
        assert!(right==1.0 && left>0.25 && left<0.35,"the far speaker stays audible, left {left}");
        assert_eq!(balance(-1.0),(right,left));
        assert_eq!(balance(f32::NAN),(1.0,1.0));
        for (cue,_) in ASSETS {
            let clip=Clip::decode(ASSETS.iter().find(|(c,_)|*c==cue).unwrap().1).unwrap();
            let centred:Vec<f32>=clip.placed(0.0).collect();
            assert_eq!(centred.len(),clip.data.len()*2/clip.channels as usize,"{cue:?} plays as stereo");
            let leaning:Vec<f32>=clip.placed(1.0).collect();
            let side=|data:&[f32],k:usize|data.iter().skip(k).step_by(2).map(|v|v*v).sum::<f32>();
            assert!(side(&leaning,0)<0.2*side(&centred,0) && side(&leaning,1)==side(&centred,1),"{cue:?}");
        }
        for cue in [Cue::Contact,Cue::Alert,Cue::Click] {assert_eq!(pitch_spread(cue),0.0);}
        assert!(pitch_spread(Cue::Explosion)>0.0 && pitch_spread(Cue::Explosion)<0.06);
        assert!(gain(Cue::PointDefence)<gain(Cue::Beam) && gain(Cue::Beam)<gain(Cue::Explosion));
        // A heavy hit on the command ship raises the alert and the hit is still heard.
        assert!(!crowded(Cue::Impact,None) && !crowded(Cue::Alert,Some(0.0)) && !crowded(Cue::Contact,Some(0.0)));
        assert!(crowded(Cue::Impact,Some(0.01)) && crowded(Cue::Beam,Some(0.074)) && !crowded(Cue::Beam,Some(0.075)));
        assert!(signal(Cue::Alert) && signal(Cue::Contact) && !signal(Cue::Impact));
    }
    #[test] fn the_drive_is_silent_at_rest_and_its_loop_has_no_seam() {
        assert_eq!(drive_voice(0.0).0,0.0);assert_eq!(drive_voice(f32::NAN).0,0.0);
        let (half,full)=(drive_voice(0.5),drive_voice(1.0));
        assert!(half.0>0.0 && half.0<full.0 && full.0<=0.6 && half.1<full.1);
        assert_eq!(drive_voice(7.0),full);
        let clip=Clip::decode(DRIVE).unwrap();
        assert_eq!((clip.channels,clip.rate),(2,48000));
        let n=clip.data.len();
        let peak=clip.data.iter().map(|v|v.abs()).fold(0.0_f32,f32::max);
        assert!(peak>0.05 && peak<0.26);
        // The step across the loop point is no larger than the steps inside it.
        let typical=(2..n).step_by(2).map(|i|(clip.data[i]-clip.data[i-2]).abs()).fold(0.0_f32,f32::max);
        assert!((clip.data[0]-clip.data[n-2]).abs()<=typical && (clip.data[1]-clip.data[n-1]).abs()<=typical);
        let mut audio=Audio::default();
        audio.drive_follow(1.0,0.1);let rising=audio.drive_level;
        assert!(rising>0.0 && rising<drive_voice(1.0).0*audio.volume,"thrust fades in");
        audio.muted=true;audio.settings_changed();assert_eq!(audio.drive_level,0.0);
    }
    #[test] fn ambient_loop_is_finite_quiet_and_stereo() {
        let s=Decoder::try_from(Cursor::new(include_bytes!("../../../assets/audio/ambient.wav").as_slice())).unwrap();
        assert_eq!(s.channels(),2);assert_eq!(s.sample_rate(),24000);
        let samples:Vec<_>=s.collect();assert_eq!(samples.len(),56*24000*2);
        assert!(samples.iter().all(|v|v.is_finite() && v.abs()<0.26));
    }
}
