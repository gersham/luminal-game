# Damage-scaled subsystem criticals

The last live cruiser battle logged 680 beam impacts, all below the old 80 TJ minimum critical threshold. No beam could roll internal damage. The player suffered one missile propulsion casualty, repaired after twenty simulated minutes.

Penetration now rolls `1 - (1 - base_chance)^(penetrating_HP / (0.01 * max_hull_HP))`. Base chance remains 20%, 40% below half hull, and 80% below quarter hull. Screens still absorb energy first; the reference damage is before armour shares the penetration. Zero penetration cannot cause a critical. The separate guaranteed missile-puncture shock retains its damaging-hit requirement.

At healthy hull, 0.5% penetration rolls 10.6%, 1% rolls 20%, and 2% rolls 36%. The test samples 100,000 independent hits at each of four damage levels, including tiny grazes.

Validation: 291 regular workspace tests pass; eight extended surveys were not rerun. Native screenshot inspected for the new helm panel. Enemy alongside regression verifies a one-light-second offset, guidance independent of unseen enemy truth, and coasting after track loss. Target selection and movement have separate regression checks.

Battle smoke survey: `luminal-cli --class-balance 3 all stock 0.35 2000`, fifteen same-class battles with stock magazines and defences. Raw results: [scaled-criticals.csv](scaled-criticals.csv). This is a small outcome sample, not a broad balance certification. Eleven battles reached beam combat and ended in reactor loss; three ended in hull loss and one timed out. Internal casualties can now decide a battle with substantial hull remaining.
