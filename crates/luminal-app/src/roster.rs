//! Authored name pools. Ten names per class and side; selection uses a separate cosmetic RNG.
use super::*;
use luminal_core::world::ShipClass;
use theme::Theme;
pub(super) const PLAYER_CLASSES:[[&str;5];4]=[
["Picket", "Frigate", "Destroyer", "Cruiser", "Battleship"],
["Patrol Monitor", "Sword Escort", "Line Destroyer", "Gothic Cruiser", "Emperor Battleship"],
["Scout Courier", "Patrol Corvette", "Fleet Destroyer", "Armoured Cruiser", "Imperial Dreadnought"],
["Light Picket Unit", "Limited Offensive Unit", "Rapid Offensive Unit", "General Offensive Unit", "Heavy Offensive Unit"],
];
pub(super) const ENEMY_CLASSES:[[&str;5];4]=[
["Corsair Scout", "Raider Frigate", "Strike Destroyer", "Corsair Cruiser", "Outlaw Dreadnought"],
["Heretic Raider", "Iconoclast Escort", "Ravager Destroyer", "Desolator Cruiser", "Despoiler Dreadnought"],
["Vargr Scout", "Vargr Corsair", "Vargr Pack Destroyer", "Vargr Strike Cruiser", "Vargr Clan Dreadnought"],
["Idiran Picket", "Idiran War Escort", "Idiran Strike Cruiser", "Idiran War Cruiser", "Idiran Command Dreadnought"],
];
const TRANSPORT_CLASSES:[&str;4]=["Freighter", "Chartist Freighter", "Subsidised Merchant", "General Transport Unit"];
const ADVERSARIES:[&str;4]=["Free Corsairs", "Ruinous Fleet", "Vargr Corsairs", "Idiran Crusade"];
const NAMES:[[[&str;10];11];4]=[
[
["Kestrel", "Tern", "Swift", "Merlin", "Petrel", "Osprey", "Kite", "Shrike", "Harrier", "Gannet"],
["Resolute", "Vigilant", "Dauntless", "Steadfast", "Gallant", "Intrepid", "Defiant", "Tenacious", "Valiant", "Sentinel"],
["Tempest", "Thunderhead", "Squall", "Maelstrom", "Lightning", "Cyclone", "Typhoon", "Tornado", "Monsoon", "Stormfront"],
["Alexandria", "Valparaiso", "Samarkand", "Timbuktu", "Cartagena", "Singapore", "Zanzibar", "Constantinople", "Marseille", "Reykjavik"],
["Concord", "Sovereignty", "Commonwealth", "Unbroken", "Ascendancy", "Citadel", "Leviathan", "Dominion", "Bulwark", "Last Argument"],
["Needle", "Cutpurse", "Jackknife", "Razorwing", "Hook", "Blackcap", "Picklock", "Shiv", "Quick Fang", "Scalpel"],
["Bad Company", "Red Hand", "Black Flag", "Loose Cannon", "Empty Promise", "Stolen Fire", "Night Toll", "False Witness", "Second Knife", "Wicked Chance"],
["Ransom", "Reprisal", "Blood Price", "No Quarter", "Dead Reckoning", "Iron Grudge", "Hard Bargain", "Broken Treaty", "Grave Intent", "Red Ledger"],
["Corsair King", "Ravenous", "Hollow Crown", "Savage Dividend", "Black Market", "Plundered Sun", "Debt Collector", "Merciless Horizon", "Brigand Prince", "Tyrants Due"],
["Pirates Parliament", "Crown of Ash", "Last Extortion", "Emperor of Nothing", "Black Dominion", "Thronebreaker", "Enemy of All", "Ruinous Fortune", "Dread Sovereign", "Kingdom of Knives"],
["Amber Road", "Long Haul", "Honest Measure", "Port Meridian", "Blue Caravan", "Cargo Manifest", "Quiet Passage", "Far Harbour", "Morning Delivery", "Homeward Bound"],
],
[
["Vesper of Duty", "Candle of Vigil", "Oathbound Watch", "Martyrs Lantern", "Pious Thorn", "Ember of Faith", "Penitent Spear", "Chapel Sentinel", "Watchers Litany", "Loyal Spur"],
["Sword of Vigilance", "Saints Reproach", "Vow of Iron", "Mercy Denied", "Pilgrims Shield", "Tithe of Blood", "Hallowed Fang", "Writ of Penance", "Unyielding Catechism", "Sanctified Wrath"],
["His Burning Judgment", "Litany of Retribution", "Emperors Rebuke", "Spear of Absolution", "Scourge of Heresy", "Testament of Fire", "Oath of the Martyred", "Sword of the Throne", "Relic of Defiance", "Decree of Ash"],
["Cathedral of Resolve", "Bastion of the Faithful", "Saints Immortal Vigil", "Sepulchre of Vengeance", "Triumph of the Throne", "Covenant of Steel", "Sovereign Benediction", "Glory of the Unbroken", "Ark of Righteous Fury", "Bulwark of Salvation"],
["Throne of Unending War", "Divine Right of Ruin", "Emperors Last Judgment", "Eternal Cathedral", "Sovereign of Martyrs", "Imperishable Dominion", "Crown of Ten Thousand Oaths", "Testament of Terra", "Bastion of the Golden Throne", "Apocalypse Sanctified"],
["Blasphemous Whisper", "Little Apostasy", "Carrion Prayer", "Thorn of Doubt", "Foul Omen", "Sinners Needle", "Wormtongue", "Defiled Candle", "Grinning Heretic", "Ragged Sacrament"],
["Broken Litany", "Saintslayer", "Joy of Desecration", "Flayed Promise", "Scorn of the Faithful", "Unholy Tithe", "Shrine of Teeth", "Hateful Benediction", "Black Communion", "Pilgrims Bane"],
["Gospel of Ruin", "Wrath Unchained", "Choir of the Damned", "Rapture of Knives", "Harvester of Oaths", "Faith Eater", "Blood for the Void", "Idol of Torment", "Unrepentant", "Prophet of Scars"],
["Cathedral of Screams", "Apostates Triumph", "Covenant of Rot", "Throne of the Forsaken", "Desecrators Crown", "Revelation of Hunger", "Grand Blasphemy", "Litany of Endless Night", "Bastion of Betrayal", "Temple of the Unmade"],
["Empire of the Hollow God", "Sovereign of Damnation", "Death of a Thousand Saints", "Eternitys Wound", "Black Sun Ascendant", "Throne Beyond Mercy", "Last Heresy", "Apotheosis of Ruin", "Crown of the Devourer", "Universe in Chains"],
["Pilgrims Burden", "Tithe Bearer", "Mercantile Benediction", "Saints Provision", "Honest Oblation", "Port of Penance", "Grain of the Faithful", "Charter of Duty", "Faithful Conveyance", "Blessed Ledger"],
],
[
["INS Farpoint", "INS Survey Twelve", "INS New Meridian", "INS Trailfinder", "INS Outbound", "INS Sounding Line", "INS Perihelion", "INS Border Light", "INS Scoutfall", "INS First Chart"],
["INS Patrolman", "INS Customs Bell", "INS Waystation", "INS Frontier Ward", "INS Free Passage", "INS Border Warden", "INS Watchbill", "INS Far Sentinel", "INS Guard Detail", "INS Safe Conduct"],
["INS Swift Reprisal", "INS Sabre", "INS Vigilant Lance", "INS Arrowhead", "INS Iron Response", "INS Cutlass", "INS Strike Order", "INS Lancepoint", "INS Resolute Spear", "INS Rapier"],
["INS Regina Ascendant", "INS Mora Vigilant", "INS Rhylanor Resolute", "INS Efate Sentinel", "INS Glisten Defender", "INS Trin Sovereign", "INS Jewell Valiant", "INS Lunion Guardian", "INS Aramis Indomitable", "INS Sylea Triumphant"],
["INS Imperial Warrant", "INS Iridium Crown", "INS Mandate of Sylea", "INS Sector Sovereign", "INS Ducal Authority", "INS Marches Unbroken", "INS Imperiums Reach", "INS Grand Admiral", "INS Warrant of Victory", "INS Throneward"],
["Quick Scent", "Sharp Ear", "Lean Hunter", "Night Runner", "Daring Cub", "Tailwind", "Hidden Paw", "Fresh Trail", "Nimble Fang", "Little Howl"],
["Lucky Bite", "Silver Muzzle", "Far Prowler", "Laughing Corsair", "Ragged Fortune", "Dancing Fang", "Bold Intruder", "Golden Snarl", "Fleet Paw", "Restless Hunter"],
["Pack Challenge", "Claim by Teeth", "Unruly Claw", "Red Pursuit", "Proud Defiance", "Howling Reprisal", "Hunters Share", "Torn Pennant", "Savage Venture", "Strongest Claim"],
["Great Hunt", "Pack Lords Prize", "Crown of Fangs", "Wandering Dominion", "Last Laughing Wolf", "Corsair Assembly", "Chiefs Triumph", "Starpack Ascendant", "Roving Sovereign", "Glory of the Hunt"],
["Ten Packs United", "High Chiefs Thunder", "Empire of the Hunt", "Thousand Fangs", "Unchallenged Alpha", "Clans of the Red Star", "Sovereign Pack", "Great Moot Ascendant", "All Trails Conquered", "Last Word of the Chief"],
["MV Jump Dividend", "MV Free Trader", "MV Honest Broker", "MV Margin of Safety", "MV Spinward Venture", "MV Mail Contract", "MV Port Expenses", "MV Subsidised Hope", "MV Freight Forward", "MV Balance Due"],
],
[
["Just Looking", "A Small Inquiry", "No Cause for Alarm", "Barely an Inconvenience", "Peripheral Interest", "Not Quite Trouble", "Let Me Check", "Passing Curiosity", "Quietly Taking Notes", "Almost Certainly Harmless"],
["A Measure of Effort", "Reasonable Precautions", "A Modest Objection", "We Can Discuss This", "An Unexpected Courtesy", "Please Reconsider", "Within Normal Tolerances", "The Polite Alternative", "A Little Persuasion", "Only If Necessary"],
["I Did Ask Nicely", "A Brief Correction", "You Were Saying", "Kinetic Diplomacy", "This Will Be Quick", "Unscheduled Emphasis", "A Point Worth Making", "Hardly Worth the Fuss", "Less Talk More Trajectory", "We Have Moved On"],
["An Abundance of Caution", "The Argument Continues", "I Have Done the Arithmetic", "A Serious Misunderstanding", "On Further Reflection", "A More Thorough Explanation", "The Limits of Courtesy", "Sufficient for the Purpose", "We Seem to Disagree", "A Carefully Measured Response"],
["Consider This a Footnote", "The Last Reasonable Option", "I Brought Supporting Evidence", "For the Avoidance of Doubt", "A Disproportionate Interest", "Let Us Settle the Matter", "I Can Explain at Length", "The End of This Discussion", "An Inconvenient Weight of Opinion", "You May Wish to Sit Down"],
["Eye of the Covenant", "Watcher of the True Path", "First Witness", "Spear of Inquiry", "Keeper of the Threshold", "Light of Certainty", "Herald of the Vow", "Hand of Observation", "Sacred Pursuit", "Vigil of the Faith"],
["Guardian of Doctrine", "Oath of the Unyielding", "Blade of Conviction", "Witness to Triumph", "Shield of the Covenant", "Scourge of Doubt", "Path of the Faithful", "Custodian of Truth", "Voice of Commandment", "Flame of Devotion"],
["Hammer of Certainty", "Wrath of the Covenant", "Sword of Final Doctrine", "Judgment of the Faithful", "Conqueror of Error", "Spear of Revelation", "Herald of Submission", "Executor of the Vow", "Burning Conviction", "Triumph of the Ordained"],
["Dominion of the True", "Pillar of Eternal Doctrine", "Unbroken Commandment", "Citadel of Revelation", "Sovereign Witness", "Will of the Covenant", "Righteous Ascendancy", "Mandate of the Faithful", "Bastion of Certitude", "Glory Beyond Question"],
["Eternal Dominion of Truth", "Throne of the Final Covenant", "All Doubt Extinguished", "Universe Under Doctrine", "Supreme Witness of Victory", "Crown of the Ordained", "The Last Commandment", "Certainty Beyond the Stars", "End of Every Heresy", "Revelation Without Limit"],
["Contents May Have Shifted", "Nothing You Need to Worry About", "A Sensible Amount of Luggage", "We Deliver Eventually", "Mind the Packaging", "Not Actually Fragile", "A Small Matter of Freight", "The Scenic Route Was Intentional", "Please Sign Somewhere", "More Room Than You Think"],
],
];
const STATION_NAMES:[[&str;10];4]=[
["Kepler Anchorage", "Tycho Relay", "Lagrange Watch", "Horizon Station", "Meridian Dock", "Faraday Array", "Orbital Haven", "Pioneer Platform", "Lunar Exchange", "Wayfarer Base"],
["Saints Vigil", "Bastion of Duty", "Shrine of the Last Watch", "Port Sanctity", "Martyrs Anchorage", "Citadel of the Faithful", "Sepulchre Gate", "Watchtower of Iron", "Chapel of the Void", "Throneward Redoubt"],
["Meridian Highport", "Depot Seventeen", "Frontier Anchorage", "Port Warrant", "Spinward Relay", "Naval Station Kestrel", "Crossroads Highport", "Ducal Waystation", "Customs Point Nine", "Marches Exchange"],
["We Are Still Here", "Keeping Up Appearances", "A Convenient Place to Stop", "Nothing Much Happens Here", "Please Mind the Orbit", "The View Is Quite Good", "Somewhere to Put Things", "We Saved You a Berth", "Quietly Going Around", "A Rather Permanent Arrangement"],
];
const STATION_CLASSES:[&str;4]=["Sensor Station","Vigil Bastion","Naval Station","Observation Hub"];
impl Theme {
    pub fn class_name(self,class:ShipClass,enemy:bool)->&'static str {
        if class==ShipClass::Transport {return TRANSPORT_CLASSES[self as usize];}
        let tier=ShipClass::COMBAT.iter().position(|c|*c==class).unwrap();
        if enemy {ENEMY_CLASSES[self as usize][tier]} else {PLAYER_CLASSES[self as usize][tier]}
    }
    pub fn adversary(self)->&'static str {ADVERSARIES[self as usize]}
    pub fn ship_name(self,class:ShipClass,enemy:bool,seed:u64,body:BodyId)->&'static str {
        let row=if class==ShipClass::Transport {10} else {ShipClass::COMBAT.iter().position(|c|*c==class).unwrap()+if enemy {5} else {0}};
        // This stream never consumes the combat/sensor random generator.
        let mut rng=luminal_core::rng::Rng::stream(seed,0x4E414D45+(self as u64)*100+body.0 as u64);
        NAMES[self as usize][row][(rng.next_u64()%10) as usize]
    }
    /// Shared civilian pool: independent of hull size or ship function.
    pub fn civilian_name(self,seed:u64,body:BodyId)->&'static str {
        self.ship_name(ShipClass::Transport,false,seed,body)
    }
    pub fn station_name(self,seed:u64,body:BodyId)->&'static str {
        let mut rng=luminal_core::rng::Rng::stream(seed,0x53544154+(self as u64)*100+body.0 as u64);
        STATION_NAMES[self as usize][(rng.next_u64()%10) as usize]
    }
    pub fn name_scenario(self,world:&mut luminal_core::world::World,seed:u64) {
        let mut index=0;
        while let Some(body)=world.body(BodyId(index)) {
            let id=BodyId(index);index+=1;
            if body.kind==BodyKind::Station {
                world.set_platform_identity(id,self.station_name(seed,id).into(),STATION_CLASSES[self as usize].into());
            } else if body.kind==BodyKind::Ship && let Some(class)=body.ship_class {
                let enemy=body.faction==RAIDER;
                let name=if !body.armed || class==ShipClass::Transport {self.civilian_name(seed,id)} else {self.ship_name(class,enemy,seed,id)};
                world.set_platform_identity(id,name.into(),self.class_name(class,enemy).into());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civilian_hulls_and_stations_use_their_shared_role_pools() {
        use luminal_core::world::{World,BodySpec};
        let specs=[BodyKind::Ship,BodyKind::Station].into_iter().enumerate().map(|(i,kind)|BodySpec {
            name:"Placeholder".into(),kind,faction:ESCORT,state:luminal_core::kinematics::State {pos:Vec2::new(i as f64*1000.0,0.0),vel:Vec2::ZERO},thrust:Vec2::ZERO,magazine:0,
        }).collect();
        let mut world=World::new(luminal_core::celestial::System {bodies:vec![]},specs,0.0,42);
        assert_eq!(world.body(BodyId(0)).unwrap().ship_class,Some(ShipClass::Frigate));
        Theme::Culture.name_scenario(&mut world,42);
        assert!(NAMES[Theme::Culture as usize][10].contains(&world.body(BodyId(0)).unwrap().name.as_str()),"an unarmed frigate is a civilian too");
        assert!(STATION_NAMES[Theme::Culture as usize].contains(&world.body(BodyId(1)).unwrap().name.as_str()));
        for theme in Theme::ALL {
            assert_eq!((0..1000).map(|seed|theme.station_name(seed,BodyId(3))).collect::<BTreeSet<_>>().len(),10);
        }
    }
    #[test]
    fn cosmetic_names_leave_simulation_evolution_unchanged() {
        let mut baseline=scenario::transport_intercept_class(42,ShipClass::Destroyer);
        baseline.advance_to(900.0);
        for theme in Theme::ALL {
            let mut world=scenario::transport_intercept_class(42,ShipClass::Destroyer);
            theme.name_scenario(&mut world,42);world.advance_to(900.0);
            for id in [BodyId(0),BodyId(1),BodyId(2)] {
                let a=baseline.body(id).unwrap();let b=world.body(id).unwrap();
                assert_eq!(a.trajectory.state_at(900.0),b.trajectory.state_at(900.0));
                assert_eq!((a.magazine,a.damage,a.thermal.heat_j),(b.magazine,b.damage,b.thermal.heat_j));
            }
        }
    }
    #[test]
    fn every_roster_has_ten_distinct_names_and_seeded_selection_varies() {
        for theme in Theme::ALL {
            for row in NAMES[theme as usize].into_iter().chain([STATION_NAMES[theme as usize]]) {
                assert_eq!(row.into_iter().collect::<BTreeSet<_>>().len(),10);
                assert!(row.iter().all(|n|!n.trim().is_empty()));
            }
            for class in ShipClass::COMBAT {
                for enemy in [false,true] {
                    let names:BTreeSet<_>=(0..1000).map(|seed|theme.ship_name(class,enemy,seed,BodyId(1))).collect();
                    assert_eq!(names.len(),10,"every entry must be selectable");
                }
            }
        }
    }
}
