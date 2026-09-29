Later changes and final 3 LS defence validation: [fire-control-and-defence.md](fire-control-and-defence.md).

# SRM guidance, laser defence, and recovery follow-up

This follows the initial balance report; its measurements used the earlier recovery and laser settings.

## Faults and changes

- Accelerating missile flight time ignored inherited closing velocity. At approximately 19,000 km/s closure and two million km separation it scheduled arrival after the launch platforms had passed. Solving range = closing velocity × time + half acceleration × time² keeps the initial SRM course forward relative to its launcher. Receding targets take longer; zero separation is handled explicitly.
- Laser half-chance range was 3,598 km, inside the SRM's 5,000 km burst envelope. It is now 7,495 km (0.025 LS). Lasers hold fire below 20% predicted hit chance and local fire-control sampling tightens to 0.1 seconds inside 0.1 LS. This prevents futile distant shots from consuming the close engagement window.
- Warship mounts now recharge independently every five seconds. Keeping the old twice-per-second rate with useful range made the initial test battles intercept essentially all SRMs.
- Hull repair is 1% per hour, six times slower. Screens recover 0.2 percentage points per minute, ten times slower. Damaged repair systems still halve hull repair; armour does not regenerate.

## Verification

The actual world/sensor/weapon path was exercised across 48 seeds: a 19,000 km/s launcher fired at a stationary, cold frigate two million km away, with two lasers, no interceptors, screens or evasion. Every initial SRM velocity gained on its launcher. Lasers destroyed 35 rounds before detonation; 13 delivered hull hits. This is a controlled last-ditch test, not a universal interception probability.

[srm-pd-followup.csv](srm-pd-followup.csv) contains three like-class battles per class, seeds 2000–2002, starting at 0.35 AU and 2,000 km/s closure:

| Class | SRM hits across 3 battles | PD kills | Finishes |
|---|---:|---:|---|
| Picket | 17 | 26 | 3 missile kills |
| Frigate | 8 | 14 | 3 beam finishes |
| Destroyer | 23 | 28 | 3 beam finishes |
| Cruiser | 81 | 0 | 3 beam finishes |
| Battleship | 25 | 60 | 3 beam finishes, including one mutual kill |

These are small regression samples. The cruiser trace explained its zero laser kills: heat was near full storage at the SRM exchange, followed by dumping and laser system destruction. Heat limits and dumping still inhibit lasers. Two battleships finished at beam/spinal range without spending their SRMs.

The stationary comparison also exposed an invalid old assertion: when both sides die, total delivered hits saturate at their hull capacity. More interception need not reduce that terminal count. The test now verifies repeatability and actual incoming missile destruction. [stationary-pd-followup.csv](stationary-pd-followup.csv) records five paired seeds: stocked ships destroyed 34–45 missiles with interceptors, while unstocked ships destroyed zero that way. The closing-frigate gate requires a damaging SRM hit exceeding 10% of frigate hull plus a subsystem critical, and subsequent damaging beam combat, instead of demanding multiple SRM hits in one random seed.

Validation: 284 regular workspace tests, seven explicitly run extended tests/surveys, and Clippy with warnings denied passed. The Omarchy installation was updated; the already-running battle was not restarted.
