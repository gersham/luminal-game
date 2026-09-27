# Stationary frigate exchange at 1 AU

**Historical baseline:** the table below predates halved offensive fuel, the
revised interceptor speed curve, boost-and-coast guidance fixes, and subsystem
damage/repair. Running the command now tests the current model, not these old
values. The current damage-model balance pass and selected magazine depth are in
[`frigate-balance.md`](frigate-balance.md); this document preserves the old baseline.

Run with `cargo run --release -p luminal-cli -- --frigate-duel 3 0,20,80`.

Fixture: two stationary, identical frigates with cold, fully raised screens,
normal point-defence lasers, and accurate initial position/velocity estimates.
Both queue their full offensive magazines (10 kinetic, 10 nuclear, 10 pumped
laser), interleaved by type, at the existing shared one-per-second launch rate.
No manoeuvres or doctrine; subsequent sensing and guidance use normal rules.
Main beams are out of range. Hits count delivered energy above 1 MJ, not hull
penetrations; screen absorption and cooling remain enabled.

| Seed | Hits on A / B | Energy on A / B (TJ) | Ships destroyed |
| --- | --- | --- | --- |
| 1000 | 10 / 12 | 300 / 600 | 0 |
| 1001 | 10 / 10 | 300 / 300 | 0 |
| 1002 | 11 / 10 | 341.418 / 300 | 0 |

Every row was identical at interceptor depths 0, 20, and 80. All nine duels
finished; neither ship launched an interceptor in any trial. This small pilot
does not establish a statistically reliable destruction probability, but does
demonstrate that changing ammunition capacity is not controlling this exchange.

The initial offensive burn budget is 59,958 km/s × 0.6 = 35,974.8 km/s (about
0.12c). Interceptor fire control rejects encounter speeds at or above 0.1c;
hit probability is also zero there. More ammunition cannot overcome that veto.
Gameplay magazine depths remain unchanged. Achieving roughly two hits per ship
requires authority to change another parameter (offensive approach speed or the
interceptor speed-effectiveness curve), then a new magazine-depth sweep in
multiples of five.
