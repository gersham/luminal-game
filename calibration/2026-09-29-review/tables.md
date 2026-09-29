# Simulation tables

Intervals are 95% Wilson intervals for independent seed outcomes within each cell.
They do not cover model or fixture uncertainty. Damage is hull plus armour HP.

## envelope

| payload | range_au | closure_kms | evade | active | n | Hit rate [95% CI] | Mean flight min |
| --- | --- | --- | --- | --- | --- | --- | --- |
| SRM | 0.014 | 0 | false | false | 256 | 98.4% [96.1, 99.4] | 6.3 |
| SRM | 0.014 | 0 | true | false | 256 | 98.4% [96.1, 99.4] | 6.3 |
| SRM | 0.14 | 0 | false | false | 256 | 80.9% [75.6, 85.2] | 19.9 |
| SRM | 0.14 | 0 | true | false | 256 | 82.8% [77.7, 86.9] | 20.0 |
| SRM | 0.14 | 0 | false | true | 256 | 89.1% [84.6, 92.3] | 19.9 |
| SRM | 0.154 | 0 | false | false | 256 | 78.1% [72.7, 82.8] | 20.9 |
| SRM | 0.168 | 0 | false | false | 256 | 73.4% [67.7, 78.5] | 21.8 |
| SRM | 0.168 | 0 | true | false | 256 | 71.9% [66.1, 77.0] | 22.0 |
| SRM | 0.14 | 5000 | false | false | 256 | 85.2% [80.3, 89.0] | 17.3 |
| SRM | 0.14 | -5000 | false | false | 256 | 0.0% [0.0, 1.5] | 22.0 |
| LRM | 0.14 | 0 | false | false | 256 | 94.9% [91.5, 97.0] | 28.1 |
| LRM | 0.14 | 0 | true | false | 256 | 96.5% [93.5, 98.1] | 28.4 |
| LRM | 1.4 | 0 | false | false | 256 | 73.8% [68.1, 78.8] | 107.2 |
| LRM | 1.4 | 0 | true | false | 256 | 0.0% [0.0, 1.5] | 117.8 |
| LRM | 1.4 | 0 | false | true | 256 | 81.2% [76.0, 85.6] | 107.2 |
| LRM | 1.54 | 0 | false | false | 256 | 69.1% [63.2, 74.5] | 115.6 |
| LRM | 1.68 | 0 | false | false | 256 | 0.0% [0.0, 1.5] | 120.0 |
| LRM | 1.68 | 0 | true | false | 256 | 0.0% [0.0, 1.5] | 120.0 |
| LRM | 1.4 | 5000 | false | false | 256 | 76.2% [70.6, 81.0] | 95.7 |
| LRM | 1.4 | -5000 | false | false | 256 | 0.0% [0.0, 1.5] | 120.0 |

## edge

| payload | range_au | closure_kms | evade | active | n | Hit rate [95% CI] | Mean flight min |
| --- | --- | --- | --- | --- | --- | --- | --- |
| SRM | 0.175 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 0.175 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 0.182 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 0.182 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 0.21 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 0.21 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 22.0 |
| LRM | 1.75 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 1.75 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 1.82 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 1.82 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 2.1 | 0 | false | false | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 2.1 | 0 | true | false | 128 | 0.0% [0.0, 2.9] | 120.0 |

## maneuver

| payload | fraction | closure_kms | target_g | n | Hit rate [95% CI] | Mean flight min |
| --- | --- | --- | --- | --- | --- | --- |
| SRM | 0.5 | 5000 | -100 | 128 | 98.4% [94.5, 99.6] | 11.4 |
| SRM | 0.5 | 0 | 0 | 128 | 99.2% [95.7, 99.9] | 14.1 |
| SRM | 0.5 | -5000 | 100 | 128 | 96.9% [92.2, 98.8] | 17.6 |
| SRM | 1 | 5000 | -100 | 128 | 82.8% [75.3, 88.4] | 17.0 |
| SRM | 1 | 0 | 0 | 128 | 79.7% [71.9, 85.7] | 19.9 |
| SRM | 1 | -5000 | 100 | 128 | 0.0% [0.0, 2.9] | 22.0 |
| SRM | 1.1 | 5000 | -100 | 128 | 68.0% [59.5, 75.4] | 18.0 |
| SRM | 1.1 | 0 | 0 | 128 | 78.9% [71.0, 85.1] | 20.9 |
| SRM | 1.1 | -5000 | 100 | 128 | 0.0% [0.0, 2.9] | 22.0 |
| LRM | 0.5 | 5000 | -100 | 128 | 89.1% [82.5, 93.4] | 56.4 |
| LRM | 0.5 | 0 | 0 | 128 | 89.8% [83.4, 94.0] | 65.3 |
| LRM | 0.5 | -5000 | 100 | 128 | 93.8% [88.2, 96.8] | 79.0 |
| LRM | 1 | 5000 | -100 | 128 | 78.1% [70.2, 84.4] | 90.6 |
| LRM | 1 | 0 | 0 | 128 | 77.3% [69.4, 83.7] | 107.2 |
| LRM | 1 | -5000 | 100 | 128 | 0.0% [0.0, 2.9] | 120.0 |
| LRM | 1.1 | 5000 | -100 | 128 | 73.4% [65.2, 80.3] | 97.3 |
| LRM | 1.1 | 0 | 0 | 128 | 71.1% [62.7, 78.2] | 115.6 |
| LRM | 1.1 | -5000 | 100 | 128 | 0.0% [0.0, 2.9] | 120.0 |

## class-evasion

| class | payload | evade | n | Hit rate [95% CI] | Mean flight min |
| --- | --- | --- | --- | --- | --- |
| Picket | SRM | false | 64 | 90.6% [81.0, 95.6] | 19.9 |
| Picket | SRM | true | 64 | 78.1% [66.6, 86.5] | 20.1 |
| Picket | LRM | false | 64 | 76.6% [64.9, 85.3] | 107.2 |
| Picket | LRM | true | 64 | 0.0% [0.0, 5.7] | 116.0 |
| Frigate | SRM | false | 64 | 90.6% [81.0, 95.6] | 19.9 |
| Frigate | SRM | true | 64 | 81.2% [70.0, 88.9] | 20.0 |
| Frigate | LRM | false | 64 | 76.6% [64.9, 85.3] | 107.2 |
| Frigate | LRM | true | 64 | 0.0% [0.0, 5.7] | 117.8 |
| Destroyer | SRM | false | 64 | 90.6% [81.0, 95.6] | 19.9 |
| Destroyer | SRM | true | 64 | 73.4% [61.5, 82.7] | 20.0 |
| Destroyer | LRM | false | 64 | 79.7% [68.3, 87.7] | 107.2 |
| Destroyer | LRM | true | 64 | 0.0% [0.0, 5.7] | 118.7 |
| Cruiser | SRM | false | 64 | 93.8% [85.0, 97.5] | 19.9 |
| Cruiser | SRM | true | 64 | 93.8% [85.0, 97.5] | 19.9 |
| Cruiser | LRM | false | 64 | 76.6% [64.9, 85.3] | 107.2 |
| Cruiser | LRM | true | 64 | 76.6% [64.9, 85.3] | 109.6 |
| Battleship | SRM | false | 64 | 93.8% [85.0, 97.5] | 19.9 |
| Battleship | SRM | true | 64 | 93.8% [85.0, 97.5] | 19.9 |
| Battleship | LRM | false | 64 | 81.2% [70.0, 88.9] | 107.2 |
| Battleship | LRM | true | 64 | 76.6% [64.9, 85.3] | 108.2 |

## Duels

| Case | Class A | n | A/B wins | Timeouts | Beam finishes | Median min | Mean SRM/LRM hits | Mean SRM/LRM/beam damage |
| --- | --- | ---: | --- | ---: | ---: | ---: | --- | --- |
| battleship-cruiser (depth 240) | Battleship | 8 | 8/0 | 0 | 8 | 181.7 | 0.4/5.6 | 4/89/5262 |
| battleship-depths (depth 80) | Battleship | 8 | 5/3 | 0 | 7 | 214.4 | 61.9/18.5 | 3681/234/11006 |
| battleship-depths (depth 120) | Battleship | 8 | 4/4 | 0 | 8 | 179.8 | 64.2/5.4 | 2682/33/8861 |
| beams (depth 2) | Picket | 12 | 0/0 | 12 | 0 | 480.0 | 0.0/0.0 | 0/0/0 |
| beams (depth 40) | Frigate | 12 | 7/5 | 0 | 12 | 13.7 | 0.0/0.0 | 0/0/1531 |
| beams (depth 80) | Destroyer | 12 | 4/8 | 0 | 12 | 10.0 | 0.0/0.0 | 0/0/1687 |
| beams (depth 200) | Cruiser | 12 | 7/5 | 0 | 12 | 10.9 | 0.0/0.0 | 0/0/5315 |
| beams (depth 240) | Battleship | 12 | 4/8 | 0 | 12 | 9.2 | 0.0/0.0 | 0/0/13378 |
| cruiser-battleship (depth 200) | Cruiser | 8 | 0/8 | 0 | 8 | 201.5 | 0.2/5.1 | 19/84/5982 |
| depleted (depth 30) | Cruiser | 8 | 4/4 | 0 | 8 | 235.0 | 24.0/32.0 | 2719/2997/1722 |
| depth-120 (depth 120) | Cruiser | 8 | 3/5 | 0 | 8 | 285.3 | 12.8/4.4 | 444/61/3779 |
| depth-80 (depth 80) | Cruiser | 8 | 5/3 | 0 | 6 | 281.7 | 25.9/12.4 | 2489/454/2294 |
| destroyer-cruiser (depth 80) | Destroyer | 8 | 0/8 | 0 | 2 | 148.1 | 15.1/8.8 | 1789/750/122 |
| destroyer-timeouts-a (depth 80) | Destroyer | 2 | 0/0 | 2 | 0 | 480.0 | 32.0/0.5 | 3099/0/0 |
| destroyer-timeouts-b (depth 80) | Destroyer | 2 | 0/0 | 2 | 0 | 480.0 | 37.0/3.5 | 3993/19/0 |
| frigate-destroyer (depth 40) | Frigate | 8 | 0/8 | 0 | 0 | 139.3 | 7.6/6.4 | 839/642/0 |
| kite (depth 200) | Cruiser | 8 | 3/5 | 0 | 8 | 292.8 | 0.2/4.5 | 4/66/4090 |
| legacy-paired (depth 200) | Cruiser | 8 | 3/5 | 0 | 8 | 302.2 | 0.0/4.4 | 0/61/5991 |
| natural-tracks-no-ping (depth 200) | Cruiser | 8 | 1/7 | 0 | 8 | 288.8 | 0.1/3.8 | 0/47/4470 |
| natural-tracks (depth 200) | Cruiser | 8 | 1/7 | 0 | 8 | 291.9 | 0.2/4.6 | 0/56/5517 |
| no-evade (depth 200) | Cruiser | 8 | 2/6 | 0 | 8 | 286.5 | 0.0/5.2 | 0/84/3781 |
| no-interceptors (depth 0) | Cruiser | 8 | 4/3 | 1 | 4 | 189.3 | 21.0/43.4 | 2385/4746/635 |
| no-ping (depth 200) | Cruiser | 8 | 5/3 | 0 | 8 | 286.3 | 0.1/4.6 | 0/52/4073 |
| opening-lrm-16h (depth 200) | Cruiser | 2 | 1/1 | 0 | 2 | 678.6 | 0.5/6.0 | 0/56/4252 |
| opening-lrm (depth 200) | Cruiser | 8 | 0/0 | 8 | 0 | 480.0 | 0.0/4.6 | 0/47/0 |
| opening-srm (depth 200) | Cruiser | 8 | 4/4 | 0 | 8 | 135.2 | 0.4/0.8 | 0/5/4298 |
| paired-stock (depth 200) | Cruiser | 8 | 5/3 | 0 | 8 | 290.2 | 0.1/4.4 | 0/61/4907 |
| retreat (depth 200) | Cruiser | 8 | 0/1 | 7 | 1 | 480.0 | 0.1/4.1 | 0/38/256 |
| rush (depth 200) | Cruiser | 8 | 2/6 | 0 | 8 | 289.2 | 0.0/5.1 | 0/70/4981 |
| stock (depth 2) | Picket | 20 | 10/9 | 0 | 0 | 182.4 | 11.9/0.0 | 1256/0/0 |
| stock (depth 40) | Frigate | 20 | 6/14 | 0 | 20 | 246.8 | 7.0/1.4 | 562/41/834 |
| stock (depth 80) | Destroyer | 20 | 9/7 | 4 | 7 | 246.1 | 29.0/2.9 | 2897/51/174 |
| stock (depth 200) | Cruiser | 20 | 11/9 | 0 | 20 | 299.5 | 0.5/4.7 | 8/54/4254 |
| stock (depth 240) | Battleship | 20 | 12/8 | 0 | 20 | 321.6 | 0.8/4.3 | 82/51/15957 |

## Timeout diagnostics

| Case | Seed | Final AU | Drive A disabled | Drive B disabled |
| --- | --- | ---: | --- | --- |
| destroyer-timeouts-a | 12004 | 0.034 | false | true |
| destroyer-timeouts-a | 12005 | 0.027 | false | true |
| destroyer-timeouts-b | 12010 | 0.004 | false | true |
| destroyer-timeouts-b | 12011 | 0.116 | true | true |
| no-interceptors | 24007 | 0.380 | true | true |
| opening-lrm | 24000 | 0.178 | false | false |
| opening-lrm | 24001 | 0.178 | false | false |
| opening-lrm | 24002 | 0.178 | false | false |
| opening-lrm | 24003 | 0.178 | false | false |
| opening-lrm | 24004 | 0.178 | false | false |
| opening-lrm | 24005 | 0.178 | false | false |
| opening-lrm | 24006 | 0.178 | false | false |
| opening-lrm | 24007 | 0.178 | false | false |
| retreat | 24000 | 0.010 | false | false |
| retreat | 24001 | 0.011 | false | false |
| retreat | 24002 | 0.010 | false | false |
| retreat | 24003 | 0.010 | false | false |
| retreat | 24004 | 0.010 | false | false |
| retreat | 24005 | 0.010 | false | false |
| retreat | 24006 | 0.010 | false | false |
