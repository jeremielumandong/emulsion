# Linux migration fidelity follow-up

RX 7700 XT, RADV Mesa 26.2.2, 2026-09-26 America/New_York.
Original generated ORA fixtures; this branch with migration fixes, uncommitted.

| Case | Pixels | Max linear | Mean linear | Max 8-bit code | >1 code | >3 codes | Edge px | >3 codes off-edge |
|---|---|---|---|---|---|---|---|---|
| vectors-500 · raster, direct · level 0 | 8294400 | 1.19e-7 | 8.05e-10 | 0 | 0.000% | 0.000% | 8.74% | 0.000% |
| vectors-500 · raster, direct · level 1 | 2073600 | 1.19e-7 | 1.25e-9 | 0 | 0.000% | 0.000% | 16.07% | 0.000% |
| vectors-500 · raster, direct · level 2 | 518400 | 1.19e-7 | 2.21e-9 | 0 | 0.000% | 0.000% | 26.12% | 0.000% |
| vectors-500 · raster, via tile cache · level 0 | 8294400 | 7.69e-6 | 1.19e-7 | 1 | 0.000% | 0.000% | 8.74% | 0.000% |
| vectors-500 · raster, via tile cache · level 1 | 2073600 | 7.69e-6 | 2.61e-7 | 1 | 0.000% | 0.000% | 16.07% | 0.000% |
| vectors-500 · raster, via tile cache · level 2 | 518400 | 7.69e-6 | 5.39e-7 | 1 | 0.000% | 0.000% | 26.12% | 0.000% |
| vectors-500 · Vello, sRGB-encoded · level 0 | 8294400 | 9.19e-1 | 2.83e-3 | 236 | 3.250% | 2.826% | 8.74% | 0.008% |
| vectors-500 · Vello, linear 8-bit · level 0 | 8294400 | 9.18e-1 | 2.96e-3 | 234 | 14.152% | 6.161% | 8.74% | 3.238% |
| layers-4k · raster, direct · level 0 | 8294400 | 7.60e-6 | 1.15e-6 | 1 | 0.000% | 0.000% | 0.00% | 0.000% |
| layers-4k · raster, direct · level 1 | 2073600 | 1.15e-3 | 1.36e-5 | 1 | 0.000% | 0.000% | 0.00% | 0.000% |
| layers-4k · raster, direct · level 2 | 518400 | 1.70e-3 | 3.81e-5 | 1 | 0.000% | 0.000% | 0.29% | 0.000% |
| layers-4k · raster, via tile cache · level 0 | 8294400 | 1.51e-5 | 6.15e-6 | 1 | 0.000% | 0.000% | 0.00% | 0.000% |
| layers-4k · raster, via tile cache · level 1 | 2073600 | 1.16e-3 | 1.85e-5 | 1 | 0.000% | 0.000% | 0.00% | 0.000% |
| layers-4k · raster, via tile cache · level 2 | 518400 | 1.71e-3 | 4.28e-5 | 1 | 0.000% | 0.000% | 0.29% | 0.000% |
