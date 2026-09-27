# System damage and independent launchers

## Current hull check (September 27, 1,000 HP restored)

Warship hull doubled from 500 to 1,000 HP; armour remains 100 HP. Repair remains
1% per ten effective minutes, now 10 HP. The historical repair figures below
do not describe the current build. No subsystem probability was changed here.

The live game log showed the raider receiving 14 impacts, including ten that
reduced hull, without a system casualty; the player received four system hits.
At 20% per penetrating hit, ten misses have about a 10.7% probability. Damage
severity does not add rolls: even a roughly 155 HP hit gets one chance. Absorbed
beam hits have 5% screen-leak chance times 20% system-hit chance (1% overall).
Enemy component displays are also snapshots from received echoes, not live truth.

Retested with the command below, seeds 1000–1004, unchanged 20% system-hit and
35% missile-puncture probabilities. Two duels lost a hull at 36.1 and 39.6 minutes;
the lost ships had 3 and 10 affected systems (0 and 2 destroyed). Three reached
the two-hour cap; the more damaged ship in each had 9 affected systems, with
2–3 destroyed. This confirms casualties work, but their per-hit randomness and
critical-system mission kills produce substantial variation. These are stationary
knife-range trials, not a reproduction of the manoeuvring live battle.

Reproduce with `cargo build --release -p luminal-cli` and
`target/release/luminal-cli --frigate-battle 5 30 0.002 10`.

Both ships start stationary with screens raised and initial sensor fixes.
Second ship begins firing ten seconds later. Current launchers are independent:
SRM 5 s, LRM 60 s; both carry 20 rounds. Interceptors remain inhibited inside 5 ls.

Propulsion and power each have twice the selection weight of any other eligible
system. Already destroyed or uninstalled systems are excluded. Per-penetration
system-hit probability was unchanged in the initial hull trials below. Armour
and screen capacity remain unchanged throughout.

500 HP trial (`systems-500hp.csv`): four hull losses with only 1–5 affected systems
on the lost ship. One duel reached the two-hour limit without hull loss.

Selected 1,000 HP trial (`systems-1000hp.csv`): first hull losses at 2,409, 2,825,
5,564 and 3,726 seconds. Lost ships had respectively 6, 6, 8 and 3 affected
systems, including one destroyed system each. A fifth duel reached two hours
without a hull loss, with a destroyed component on one survivor. These results
mean more degradation before hull destruction, not numerous guaranteed destroyed
systems: a single critical-system loss can stop effective combat.

## Final doubled-chance pass

`systems-double-chance.csv` uses 20% penetrating subsystem-hit chance (previously
10%) and 35% missile screen-puncture chance (previously 17.5%). The same five
seeds produced two hull losses at 2,201 and 2,444 seconds, with 3 and 10 affected
systems on those lost ships (0 and 2 destroyed). Three trials reached the two-hour
limit without hull destruction; the more-damaged survivor in each had 9 affected
systems, including 2–3 destroyed. Early critical-system failure can end effective
combat well before either hull is gone. Damage is not scripted to a casualty count.

Small deterministic seed samples; not a guarantee for manoeuvring player battles.
Repair stays at 10% maximum hull/hour, so raising hull also raises absolute HP
repaired/hour. Armour does not regenerate.
