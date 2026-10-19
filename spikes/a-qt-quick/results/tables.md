
## Conditions

| condition | refresh (exact) | runs | background CPU % (min–max) | power plan |
|---|---|---|---|---|
| 144 Hz / 100 % | 143.856 Hz | 109 | 0.6–12.2 | High performance, Power saver |
| 60 Hz / 100 % | 59.951 Hz | 85 | 1.2–23.3 | High performance, Power saver |
| 144 Hz / 125 % | 143.856 Hz | 38 | 0.7–13.0 | High performance |
| 60 Hz / 125 % | 59.951 Hz | 38 | 0.9–12.8 | High performance |
| 144 Hz / 150 % | 143.856 Hz | 38 | 0.9–11.8 | High performance |
| 60 Hz / 150 % | 59.951 Hz | 38 | 1.1–17.9 | High performance |
| 144 Hz / 100 % · D3D12 | 143.856 Hz | 97 | 0.7–21.0 | High performance, Power saver |
| 60 Hz / 100 % · D3D12 | 59.951 Hz | 38 | 1.4–28.0 | High performance |
| 144 Hz / 125 % · D3D12 | 143.856 Hz | 38 | 0.9–11.1 | High performance |
| 144 Hz / 150 % · D3D12 | 143.856 Hz | 38 | 0.8–13.8 | High performance |

## Positive control

| condition | during | pass | detected | injected → measured (ms), GUI thread | render thread |
|---|---|---|---|---|---|
| 144 Hz / 100 % | animation | PresentMon | 10/10 | 4→4.2, 8→8.0, 16→16.1, 33→33.2, 100→100.2 | 4→4.2, 8→8.0, 16→15.8, 33→33.1, 100→100.4 |
| 144 Hz / 100 % | animation | duplication | 7/10 | 4→0.3, 8→0.3, 16→13.9, 33→28.1, 100→97.8 | 4→0.1, 8→7.1, 16→7.0, 33→27.8, 100→90.9 |
| 144 Hz / 100 % | scroll | PresentMon | 10/10 | 4→3.9, 8→8.2, 16→16.1, 33→32.6, 100→100.0 | 4→4.2, 8→8.1, 16→14.8, 33→33.4, 100→100.2 |
| 144 Hz / 100 % | scroll | duplication | 3/10 | 4→0.3, 8→7.0, 16→7.0, 33→21.0, 100→90.6 | 4→0.2, 8→0.3, 16→6.6, 33→20.6, 100→90.8 |
| 60 Hz / 100 % | animation | PresentMon | 10/10 | 4→3.9, 8→8.0, 16→16.0, 33→33.1, 100→100.1 | 4→4.1, 8→8.5, 16→16.2, 33→33.1, 100→100.0 |
| 60 Hz / 100 % | animation | duplication | 10/10 | 4→0.2, 8→0.3, 16→0.2, 33→33.9, 100→83.7 | 4→0.7, 8→0.9, 16→17.1, 33→17.2, 100→83.9 |
| 60 Hz / 100 % | scroll | PresentMon | 10/10 | 4→5.0, 8→7.1, 16→16.4, 33→32.6, 100→100.3 | 4→3.6, 8→8.4, 16→15.4, 33→33.3, 100→99.8 |
| 60 Hz / 100 % | scroll | duplication | 10/10 | 4→0.1, 8→0.1, 16→17.0, 33→16.8, 100→83.7 | 4→0.3, 8→0.3, 16→0.2, 33→16.8, 100→84.0 |
| 144 Hz / 125 % | animation | PresentMon | 10/10 | 4→4.2, 8→8.1, 16→16.1, 33→33.2, 100→100.1 | 4→4.1, 8→8.3, 16→16.2, 33→33.1, 100→100.0 |
| 144 Hz / 125 % | animation | duplication | 6/10 | 4→0.3, 8→0.2, 16→14.1, 33→27.8, 100→97.5 | 4→0.3, 8→6.9, 16→6.9, 33→21.0, 100→90.5 |
| 144 Hz / 125 % | scroll | PresentMon | 10/10 | 4→4.3, 8→8.1, 16→16.0, 33→33.1, 100→100.2 | 4→3.9, 8→8.3, 16→15.9, 33→33.7, 100→100.3 |
| 144 Hz / 125 % | scroll | duplication | 4/10 | 4→0.2, 8→7.1, 16→6.7, 33→21.0, 100→90.6 | 4→0.3, 8→0.3, 16→7.0, 33→28.0, 100→90.7 |
| 60 Hz / 125 % | animation | PresentMon | 10/10 | 4→4.3, 8→8.1, 16→16.1, 33→32.9, 100→100.1 | 4→4.1, 8→8.0, 16→16.1, 33→33.3, 100→100.2 |
| 60 Hz / 125 % | animation | duplication | 9/10 | 4→0.3, 8→0.3, 16→16.8, 33→16.8, 100→83.3 | 4→0.2, 8→0.2, 16→17.4, 33→16.8, 100→83.5 |
| 60 Hz / 125 % | scroll | PresentMon | 10/10 | 4→4.5, 8→7.7, 16→17.1, 33→33.5, 100→99.7 | 4→4.5, 8→8.0, 16→16.0, 33→33.2, 100→99.8 |
| 60 Hz / 125 % | scroll | duplication | 10/10 | 4→0.3, 8→0.3, 16→16.8, 33→16.9, 100→83.7 | 4→0.2, 8→0.2, 16→0.4, 33→16.6, 100→83.8 |
| 144 Hz / 150 % | animation | PresentMon | 10/10 | 4→4.2, 8→8.0, 16→16.0, 33→33.2, 100→100.2 | 4→4.1, 8→8.0, 16→16.3, 33→32.8, 100→99.9 |
| 144 Hz / 150 % | animation | duplication | 6/10 | 4→0.3, 8→0.3, 16→13.9, 33→27.9, 100→97.8 | 4→0.2, 8→7.0, 16→6.9, 33→21.0, 100→90.3 |
| 144 Hz / 150 % | scroll | PresentMon | 10/10 | 4→3.8, 8→8.0, 16→16.1, 33→32.7, 100→100.2 | 4→4.2, 8→7.9, 16→15.9, 33→32.5, 100→100.7 |
| 144 Hz / 150 % | scroll | duplication | 3/10 | 4→0.1, 8→6.8, 16→7.0, 33→21.0, 100→90.6 | 4→0.2, 8→0.3, 16→7.0, 33→20.9, 100→90.3 |
| 60 Hz / 150 % | animation | PresentMon | 10/10 | 4→4.4, 8→8.1, 16→16.1, 33→33.2, 100→100.1 | 4→4.1, 8→7.8, 16→16.1, 33→33.1, 100→100.2 |
| 60 Hz / 150 % | animation | duplication | 10/10 | 4→0.1, 8→0.2, 16→16.5, 33→17.0, 100→83.7 | 4→0.3, 8→0.2, 16→16.9, 33→16.7, 100→83.7 |
| 60 Hz / 150 % | scroll | PresentMon | 10/10 | 4→4.7, 8→7.4, 16→16.7, 33→32.9, 100→101.0 | 4→4.2, 8→7.4, 16→16.0, 33→32.6, 100→100.2 |
| 60 Hz / 150 % | scroll | duplication | 10/10 | 4→0.2, 8→0.1, 16→17.1, 33→16.7, 100→83.5 | 4→1.0, 8→0.2, 16→0.2, 33→16.7, 100→83.6 |
| 144 Hz / 100 % · D3D12 | animation | PresentMon | 10/10 | 4→4.3, 8→8.0, 16→16.2, 33→33.2, 100→100.3 | 4→4.1, 8→8.0, 16→15.9, 33→33.3, 100→100.1 |
| 144 Hz / 100 % · D3D12 | animation | duplication | 6/10 | 4→0.5, 8→0.3, 16→13.9, 33→28.3, 100→97.8 | 4→0.3, 8→7.0, 16→7.0, 33→20.7, 100→90.9 |
| 144 Hz / 100 % · D3D12 | scroll | PresentMon | 10/10 | 4→3.8, 8→8.1, 16→16.5, 33→33.1, 100→100.2 | 4→4.2, 8→8.2, 16→15.7, 33→33.3, 100→100.3 |
| 144 Hz / 100 % · D3D12 | scroll | duplication | 3/10 | 4→0.3, 8→7.3, 16→6.8, 33→20.7, 100→90.8 | 4→0.2, 8→0.2, 16→7.1, 33→20.7, 100→90.8 |
| 60 Hz / 100 % · D3D12 | animation | PresentMon | 10/10 | 4→4.1, 8→8.1, 16→16.1, 33→33.3, 100→100.2 | 4→4.0, 8→7.5, 16→16.2, 33→33.0, 100→100.2 |
| 60 Hz / 100 % · D3D12 | animation | duplication | 10/10 | 4→0.2, 8→0.1, 16→16.5, 33→17.0, 100→83.5 | 4→0.3, 8→0.3, 16→16.8, 33→16.6, 100→83.7 |
| 60 Hz / 100 % · D3D12 | scroll | PresentMon | 10/10 | 4→3.8, 8→8.7, 16→16.1, 33→32.7, 100→100.6 | 4→4.0, 8→8.0, 16→15.8, 33→33.0, 100→100.0 |
| 60 Hz / 100 % · D3D12 | scroll | duplication | 10/10 | 4→0.3, 8→0.1, 16→16.9, 33→16.6, 100→83.6 | 4→0.4, 8→0.3, 16→0.3, 33→17.0, 100→83.4 |
| 144 Hz / 125 % · D3D12 | animation | PresentMon | 10/10 | 4→4.3, 8→8.0, 16→16.1, 33→33.0, 100→100.1 | 4→4.2, 8→8.0, 16→15.9, 33→33.0, 100→99.7 |
| 144 Hz / 125 % · D3D12 | animation | duplication | 7/10 | 4→0.2, 8→6.9, 16→13.8, 33→27.9, 100→97.7 | 4→0.2, 8→7.2, 16→6.9, 33→21.0, 100→90.5 |
| 144 Hz / 125 % · D3D12 | scroll | PresentMon | 10/10 | 4→4.1, 8→8.2, 16→16.2, 33→32.8, 100→101.2 | 4→3.9, 8→8.2, 16→16.1, 33→33.2, 100→100.8 |
| 144 Hz / 125 % · D3D12 | scroll | duplication | 3/10 | 4→0.3, 8→6.7, 16→7.3, 33→20.9, 100→90.9 | 4→0.1, 8→0.3, 16→6.8, 33→20.8, 100→90.7 |
| 144 Hz / 150 % · D3D12 | animation | PresentMon | 10/10 | 4→4.2, 8→8.0, 16→16.1, 33→33.1, 100→100.2 | 4→4.0, 8→8.1, 16→16.1, 33→33.1, 100→100.2 |
| 144 Hz / 150 % · D3D12 | animation | duplication | 6/10 | 4→0.1, 8→0.2, 16→13.9, 33→27.9, 100→97.4 | 4→0.2, 8→6.9, 16→6.8, 33→20.9, 100→90.8 |
| 144 Hz / 150 % · D3D12 | scroll | PresentMon | 10/10 | 4→4.1, 8→8.3, 16→16.0, 33→33.1, 100→101.1 | 4→2.9, 8→8.0, 16→16.2, 33→31.7, 100→99.9 |
| 144 Hz / 150 % · D3D12 | scroll | duplication | 3/10 | 4→0.3, 8→7.2, 16→6.9, 33→20.9, 100→90.7 | 4→0.2, 8→0.3, 16→7.0, 33→21.0, 100→90.7 |

## Knees

| axis | 144 Hz / 100 % | 60 Hz / 100 % | 144 Hz / 125 % | 60 Hz / 125 % | 144 Hz / 150 % | 60 Hz / 150 % | 144 Hz / 100 % · D3D12 | 60 Hz / 100 % · D3D12 | 144 Hz / 125 % · D3D12 | 144 Hz / 150 % · D3D12 |
|---|---|---|---|---|---|---|---|---|---|---|
| artwork | **rows 10000, weight 1, uncached art ×1, fling:8000** (7.45/min); display: **rows 10000, weight 1, uncached art ×1, fling:8000** (14.9/min) | no drops; display: no drops | – | – | – | – | no drops; display: **rows 10000, weight 1, fling:8000** (14.9/min) | – | – | – |
| churn | **rows 10000, weight 1, churn/s 10, fling:8000** (7.45/min); display: **rows 10000, weight 1, churn/s 1, fling:8000** (14.9/min) | no drops; display: no drops | – | – | – | – | **rows 10000, weight 1, churn/s 10, fling:8000** (22.36/min); display: **rows 10000, weight 1, churn/s 1, fling:8000** (14.9/min) | – | – | – |
| combined | **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (1471.19/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (7.47/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (1179.94/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (642.27/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (22.4/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (52.28/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (1008.18/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (500.37/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (14.94/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (29.87/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (515.29/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000** (477.97/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (7.47/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (29.87/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000** (1142.61/min); display: **rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000** (261.39/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (37.34/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (44.81/min) | **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (14.94/min); display: **rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000** (14.94/min) |
| cpu | **rows 10000, weight 1, fling:8000** (7.45/min); display: **rows 10000, weight 1, fling:8000** (7.45/min) | no drops; display: no drops | – | – | – | – | no drops; display: **rows 10000, weight 1, fling:8000** (7.45/min) | – | – | – |
| ext_churn | – | – | – | – | – | – | **rows 10000, weight 1, churn/s 3000, fling:8000** (208.67/min); display: **rows 10000, weight 1, churn/s 3000, fling:8000** (44.71/min) | – | – | – |
| ext_combined | – | – | – | – | – | – | **rows 10000000, weight 2, churn/s 1000, uncached art ×4, affinity 0x1, power saver, fling:12000** (6459.86/min); display: **rows 10000000, weight 2, churn/s 1000, uncached art ×4, affinity 0x1, power saver, fling:12000** (6228.53/min) | – | – | – |
| ext_cpu | – | – | – | – | – | – | **rows 10000, weight 2, affinity 0x1, fling:12000** (14.94/min); display: **rows 10000, weight 2, affinity 0x1, fling:12000** (22.4/min) | – | – | – |
| ext_rows | – | – | – | – | – | – | no drops; display: **rows 10000000, weight 1, fling:12000** (37.34/min) | – | – | – |
| fling | **rows 10000, weight 1, fling:4000** (7.41/min); display: **rows 10000, weight 1, fling:1000** (14.28/min) | no drops; display: no drops | **rows 10000, weight 1, fling:1000** (7.14/min); display: **rows 10000, weight 1, fling:1000** (49.99/min) | no drops; display: no drops | **rows 10000, weight 1, fling:2000** (7.32/min); display: **rows 10000, weight 1, fling:1000** (42.85/min) | no drops; display: no drops | no drops; display: **rows 10000, weight 1, fling:1000** (14.28/min) | no drops; display: no drops | no drops; display: **rows 10000, weight 1, fling:1000** (14.28/min) | no drops; display: **rows 10000, weight 1, fling:1000** (7.14/min) |
| gpu | **rows 10000, weight 1, iGPU, fling:8000** (104.34/min); display: **rows 10000, weight 1, iGPU, fling:8000** (104.33/min) | **rows 10000, weight 1, WARP, fling:8000** (365.17/min); display: no drops | **rows 10000, weight 1, fling:8000** (14.91/min); display: **rows 10000, weight 1, fling:8000** (37.26/min) | **rows 10000, weight 1, WARP, fling:8000** (327.91/min); display: no drops | **rows 10000, weight 1, fling:8000** (22.36/min); display: **rows 10000, weight 1, fling:8000** (74.52/min) | **rows 10000, weight 1, WARP, fling:8000** (469.51/min); display: no drops | **rows 10000, weight 1, iGPU, fling:8000** (111.79/min); display: **rows 10000, weight 1, fling:8000** (37.26/min) | **rows 10000, weight 1, WARP, fling:8000** (394.98/min); display: no drops | **rows 10000, weight 1, iGPU, fling:8000** (22.36/min); display: **rows 10000, weight 1, fling:8000** (22.36/min) | **rows 10000, weight 1, iGPU, fling:8000** (14.91/min); display: **rows 10000, weight 1, fling:8000** (14.9/min) |
| llvmpipe_hw_control | **rows 1000, weight 1, fling:2000** (28.7/min) | **rows 1000, weight 1, wheel:10** (7.5/min) | **rows 1000, weight 1, fling:2000** (95.9/min) | **rows 1000, weight 1, fling:2000** (52.5/min) | **rows 1000, weight 1, fling:2000** (24.9/min) | **rows 1000, weight 1, fling:2000** (31.8/min) | **rows 1000, weight 1, fling:2000** (68.0/min) | **rows 1000, weight 1, fling:8000** (73.5/min) | **rows 1000, weight 1, fling:2000** (30.4/min) | **rows 1000, weight 1, wheel:10** (7.4/min) |
| llvmpipe_production | **rows 1000, weight 1, fling:2000** (1148.0/min) | **rows 1000, weight 1, fling:2000** (0.0/min) | **rows 1000, weight 1, fling:2000** (1173.8/min) | **rows 1000, weight 1, fling:2000** (0.0/min) | **rows 1000, weight 1, fling:2000** (2595.8/min) | **rows 1000, weight 1, fling:2000** (0.0/min) | **rows 1000, weight 1, fling:2000** (1256.8/min) | **rows 1000, weight 1, fling:2000** (0.0/min) | **rows 1000, weight 1, fling:2000** (1943.5/min) | **rows 1000, weight 1, fling:2000** (2218.5/min) |
| rows | **rows 10000, weight 1, fling:8000** (7.45/min); display: **rows 1000, weight 1, fling:8000** (22.36/min) | no drops; display: no drops | **rows 1000, weight 1, fling:8000** (7.45/min); display: **rows 1000, weight 1, fling:8000** (14.9/min) | no drops; display: no drops | **rows 10000, weight 1, fling:8000** (14.91/min); display: **rows 1000, weight 1, fling:8000** (37.26/min) | no drops; display: no drops | no drops; display: **rows 1000, weight 1, fling:8000** (44.71/min) | no drops; display: no drops | no drops; display: **rows 1000, weight 1, fling:8000** (14.9/min) | no drops; display: **rows 1000, weight 1, fling:8000** (14.9/min) |
| touchpad | **rows 10000, weight 1, touchpad:1000** (26.55/min); display: **rows 10000, weight 1, touchpad:1000** (53.09/min) | no drops; display: no drops | – | – | – | – | no drops; display: **rows 10000, weight 1, touchpad:1000** (13.27/min) | – | – | – |
| warp_production | **rows 1000, weight 1, WARP, fling:2000** (438.97/min); display: **rows 1000, weight 1, WARP, fling:2000** (21.95/min) | **rows 1000, weight 1, WARP, fling:2000** (139.01/min); display: no drops | – | – | – | – | **rows 1000, weight 1, WARP, fling:2000** (460.92/min); display: **rows 1000, weight 1, WARP, fling:2000** (80.48/min) | – | – | – |
| weight | **rows 10000, weight 3, fling:8000** (7.45/min); display: **rows 10000, weight 0, fling:8000** (7.45/min) | no drops; display: no drops | **rows 10000, weight 0, fling:8000** (14.91/min); display: **rows 10000, weight 0, fling:8000** (22.36/min) | no drops; display: no drops | **rows 10000, weight 0, fling:8000** (7.45/min); display: **rows 10000, weight 0, fling:8000** (22.36/min) | no drops; display: no drops | **rows 10000, weight 1, fling:8000** (14.91/min); display: **rows 10000, weight 0, fling:8000** (14.9/min) | **rows 10000, weight 2, fling:8000** (7.45/min); display: no drops | **rows 10000, weight 2, fling:8000** (29.81/min); display: **rows 10000, weight 0, fling:8000** (29.81/min) | **rows 10000, weight 2, fling:8000** (22.36/min); display: **rows 10000, weight 0, fling:8000** (14.9/min) |
| wheel | **rows 10000, weight 1, wheel:5** (33.9/min); display: **rows 10000, weight 1, wheel:5** (88.15/min) | no drops; display: no drops | – | – | – | – | no drops; display: **rows 10000, weight 1, wheel:5** (6.78/min) | – | – | – |

## Queue

| condition | transitions | edits/s configured (measured) | PresentMon dropped/min presents [display] (p99, max ms) | frames checked (settled) | corruption frames | still broken after edits stop | insert/remove input-to-photon p50/p99 ms |
|---|---|---|---|---|---|---|---|
| 144 Hz / 100 % | settling | 1 (1) | 22.06 [26.47] (7.5, 14.1) | 2264 (1517) | 0  | 0 | 3.9/21.8 |
| 144 Hz / 100 % | settling | 5 (5) | 30.88 [52.94] (7.7, 14.0) | 2262 (900) | 0  | 0 | 5.0/24.1 |
| 144 Hz / 100 % | settling | 20 (19) | 39.71 [75.0] (7.8, 14.1) | 2261 (817) | 1 (ghost 1) | 0 | 5.6/28.7 |
| 144 Hz / 100 % | settling | 50 (48) | 17.65 [35.29] (7.8, 13.9) | 2267 (809) | 160 (order 80, gap 80) | 0 | 3.8/22.4 |
| 144 Hz / 100 % | settling | 100 (120) | 48.53 [66.18] (8.2, 14.6) | 2259 (803) | 782 (order 391, gap 391) | 288 | 4.3/24.5 |
| 144 Hz / 100 % | settling | 200 (181) | 35.29 [70.59] (8.3, 14.3) | 2263 (808) | 160 (order 80, gap 80) | 0 | 5.5/21.7 |
| 144 Hz / 100 % | plain | 1 (1) | 35.29 [57.35] (7.5, 14.1) | 2265 (1517) | 0  | 0 | 5.1/35.2 |
| 144 Hz / 100 % | plain | 5 (4) | 26.47 [39.71] (7.5, 14.1) | 2260 (903) | 934 (order 467, gap 467) | 282 | 5.0/25.0 |
| 144 Hz / 100 % | plain | 20 (19) | 39.71 [52.94] (7.8, 14.3) | 2261 (819) | 808 (order 404, gap 404) | 288 | 4.0/23.0 |
| 60 Hz / 100 % | settling | 1 (1) | 0.0 [0.0] (17.3, 17.5) | 947 (621) | 0  | 0 | 2.7/35.6 |
| 60 Hz / 100 % | settling | 5 (5) | 0.0 [0.0] (17.7, 18.3) | 947 (369) | 0  | 0 | 6.5/39.6 |
| 60 Hz / 100 % | settling | 20 (19) | 0.0 [0.0] (17.4, 17.9) | 944 (339) | 0  | 0 | 2.3/35.1 |
| 60 Hz / 100 % | settling | 50 (48) | 0.0 [0.0] (17.5, 18.1) | 947 (335) | 62 (order 31, gap 31) | 0 | 6.5/39.2 |
| 60 Hz / 100 % | settling | 100 (116) | 0.0 [0.0] (17.6, 18.6) | 947 (334) | 62 (order 31, gap 31) | 0 | 6.3/38.9 |
| 60 Hz / 100 % | settling | 200 (174) | 0.0 [0.0] (17.6, 19.9) | 947 (334) | 62 (order 31, gap 31) | 0 | 8.1/41.0 |
| 60 Hz / 100 % | plain | 1 (1) | 0.0 [0.0] (17.3, 18.1) | 944 (599) | 0  | 0 | 15.2/48.0 |
| 60 Hz / 100 % | plain | 5 (5) | 0.0 [0.0] (17.4, 17.9) | 948 (357) | 348 (order 174, gap 174) | 120 | 8.2/41.1 |
| 60 Hz / 100 % | plain | 20 (19) | 0.0 [0.0] (17.4, 18.0) | 947 (337) | 326 (order 163, gap 163) | 120 | 3.0/36.5 |
| 144 Hz / 125 % | settling | 5 (5) | 52.94 [88.24] (7.7, 13.9) | 2268 (901) | 0  | 0 | 3.5/22.2 |
| 144 Hz / 125 % | settling | 20 (19) | 30.88 [57.35] (7.7, 14.0) | 2264 (818) | 1 (ghost 1) | 0 | 4.6/23.3 |
| 144 Hz / 125 % | settling | 50 (48) | 52.94 [105.88] (7.8, 14.5) | 2268 (809) | 160 (order 80, gap 80) | 0 | 4.0/23.3 |
| 144 Hz / 125 % | plain | 5 (4) | 48.53 [88.24] (7.7, 14.4) | 2267 (905) | 940 (order 470, gap 470) | 288 | 4.2/27.6 |
| 60 Hz / 125 % | settling | 5 (5) | 0.0 [0.0] (17.6, 18.1) | 945 (354) | 0  | 0 | 15.8/48.6 |
| 60 Hz / 125 % | settling | 20 (20) | 0.0 [0.0] (17.6, 20.0) | 947 (332) | 0  | 0 | 15.0/48.0 |
| 60 Hz / 125 % | settling | 50 (48) | 0.0 [0.0] (17.8, 18.1) | 947 (335) | 62 (order 31, gap 31) | 0 | 7.4/40.2 |
| 60 Hz / 125 % | plain | 5 (5) | 0.0 [0.0] (17.5, 20.2) | 946 (356) | 348 (order 174, gap 174) | 120 | 9.6/42.5 |
| 144 Hz / 150 % | settling | 5 (4) | 75.0 [105.88] (7.8, 14.2) | 2270 (907) | 0  | 0 | 5.3/24.3 |
| 144 Hz / 150 % | settling | 20 (19) | 57.35 [101.47] (7.8, 14.1) | 2270 (821) | 1 (ghost 1) | 0 | 5.1/24.1 |
| 144 Hz / 150 % | settling | 50 (48) | 79.41 [114.71] (8.3, 14.2) | 2266 (808) | 162 (order 81, gap 81) | 0 | 5.0/25.1 |
| 144 Hz / 150 % | plain | 5 (5) | 39.71 [57.35] (7.7, 14.5) | 2267 (905) | 936 (order 468, gap 468) | 284 | 4.6/24.4 |
| 60 Hz / 150 % | settling | 5 (5) | 0.0 [0.0] (17.7, 17.9) | 947 (355) | 0  | 0 | 12.1/45.0 |
| 60 Hz / 150 % | settling | 20 (20) | 0.0 [0.0] (17.6, 18.0) | 948 (334) | 0  | 0 | 16.8/49.7 |
| 60 Hz / 150 % | settling | 50 (48) | 0.0 [0.0] (17.6, 18.1) | 945 (332) | 0  | 0 | 16.2/49.1 |
| 60 Hz / 150 % | plain | 5 (4) | 0.0 [0.0] (17.5, 17.8) | 947 (369) | 372 (order 186, gap 186) | 120 | 4.6/37.7 |
| 144 Hz / 100 % · D3D12 | settling | 1 (1) | 13.24 [26.47] (7.4, 18.5) | 2268 (1521) | 0  | 0 | 5.4/26.1 |
| 144 Hz / 100 % · D3D12 | settling | 5 (5) | 0.0 [26.47] (7.6, 8.1) | 2271 (908) | 0  | 0 | 4.5/26.8 |
| 144 Hz / 100 % · D3D12 | settling | 20 (19) | 0.0 [13.24] (7.7, 8.2) | 2271 (821) | 1 (ghost 1) | 0 | 4.6/25.1 |
| 144 Hz / 100 % · D3D12 | settling | 50 (48) | 0.0 [22.06] (7.7, 8.7) | 2272 (810) | 0  | 0 | 3.8/22.8 |
| 144 Hz / 100 % · D3D12 | settling | 100 (120) | 0.0 [39.71] (7.9, 8.5) | 2271 (809) | 798 (order 399, gap 399) | 286 | 5.0/17.0 |
| 144 Hz / 100 % · D3D12 | settling | 200 (181) | 4.41 [39.71] (8.1, 13.6) | 2273 (810) | 164 (order 82, gap 82) | 0 | 3.5/22.4 |
| 144 Hz / 100 % · D3D12 | plain | 1 (1) | 0.0 [22.06] (7.4, 8.7) | 2269 (1488) | 0  | 0 | 5.3/24.2 |
| 144 Hz / 100 % · D3D12 | plain | 5 (4) | 4.41 [30.88] (7.5, 13.9) | 2271 (908) | 944 (order 472, gap 472) | 288 | 4.6/27.6 |
| 144 Hz / 100 % · D3D12 | plain | 20 (19) | 4.41 [39.71] (7.6, 13.7) | 2269 (820) | 812 (order 406, gap 406) | 288 | 4.1/23.2 |
| 60 Hz / 100 % · D3D12 | settling | 5 (5) | 0.0 [0.0] (17.4, 18.0) | 948 (353) | 0  | 0 | 12.8/45.5 |
| 60 Hz / 100 % · D3D12 | settling | 20 (20) | 0.0 [0.0] (17.4, 17.8) | 948 (334) | 0  | 0 | 16.7/49.8 |
| 60 Hz / 100 % · D3D12 | settling | 50 (48) | 0.0 [0.0] (17.5, 18.1) | 946 (335) | 62 (order 31, gap 31) | 0 | 2.2/35.3 |
| 60 Hz / 100 % · D3D12 | plain | 5 (5) | 0.0 [0.0] (17.4, 17.6) | 947 (353) | 348 (order 174, gap 174) | 120 | 14.2/47.2 |
| 144 Hz / 125 % · D3D12 | settling | 5 (5) | 0.0 [22.06] (7.6, 8.3) | 2271 (907) | 0  | 0 | 4.9/27.1 |
| 144 Hz / 125 % · D3D12 | settling | 20 (19) | 0.0 [17.65] (7.6, 8.8) | 2267 (821) | 1 (ghost 1) | 0 | 5.5/28.5 |
| 144 Hz / 125 % · D3D12 | settling | 50 (48) | 0.0 [17.65] (7.7, 8.6) | 2270 (812) | 164 (order 82, gap 82) | 0 | 5.5/24.6 |
| 144 Hz / 125 % · D3D12 | plain | 5 (5) | 0.0 [35.29] (7.5, 8.1) | 2272 (909) | 942 (order 471, gap 471) | 288 | 4.4/26.4 |
| 144 Hz / 150 % · D3D12 | settling | 5 (4) | 0.0 [26.47] (7.5, 8.1) | 2269 (907) | 0  | 0 | 4.2/26.3 |
| 144 Hz / 150 % · D3D12 | settling | 20 (19) | 0.0 [13.24] (7.7, 8.0) | 2273 (822) | 0  | 0 | 3.8/22.9 |
| 144 Hz / 150 % · D3D12 | settling | 50 (48) | 0.0 [13.24] (7.6, 7.9) | 2268 (811) | 164 (order 82, gap 82) | 0 | 4.9/24.1 |
| 144 Hz / 150 % · D3D12 | plain | 5 (5) | 0.0 [8.82] (7.5, 8.1) | 2268 (880) | 886 (order 443, gap 443) | 288 | 5.3/25.3 |

## Sidebar and pages

| condition | scenario | load | PresentMon dropped/min presents [display] | p99 / max frame ms | input-to-photon p50 / p99 ms |
|---|---|---|---|---|---|
| 144 Hz / 100 % | sidebar | rows 1000, weight 1 | 41.23 [61.85] | 7.6 / 14.1 | 11.9 / 14.1 |
| 144 Hz / 100 % | sidebar | rows 1000000, weight 1 | 6.87 [13.74] | 7.4 / 13.8 | 11.8 / 14.3 |
| 144 Hz / 100 % | sidebar | rows 1000000, weight 2 | 13.74 [20.62] | 7.6 / 14.0 | 18.9 / 21.7 |
| 144 Hz / 100 % | sidebar | rows 1000000, weight 2, affinity 0x1 | 164.93 [130.57] | 13.9 / 20.9 | 25.5 / 28.0 |
| 144 Hz / 100 % | sidebar | rows 1000000, weight 2, WARP | 343.61 [316.11] | 20.4 / 22.2 | 39.0 / 41.9 |
| 144 Hz / 100 % | sidebar | rows 1000000, weight 2, WARP, affinity 0x1 | 4350.11 [4219.45] | 39.5 / 46.2 | 75.3 / 80.7 |
| 144 Hz / 100 % | pages | rows 10000, weight 1, fresh 0 | 10.98 [27.44] | 7.4 / 14.1 | 11.7 / 20.6 |
| 144 Hz / 100 % | pages | rows 10000, weight 2, fresh 0 | 16.47 [38.42] | 7.4 / 14.1 | 18.6 / 21.4 |
| 144 Hz / 100 % | pages | rows 10000, weight 1, fresh 1 | 54.89 [60.38] | 8.1 / 14.0 | 18.5 / 22.0 |
| 144 Hz / 100 % | pages | rows 10000, weight 2, fresh 1 | 104.29 [71.36] | 11.2 / 20.7 | 22.0 / 24.3 |
| 144 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, uncached art ×4 | 126.25 [98.8] | 11.8 / 22.4 | 20.6 / 22.8 |
| 144 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, affinity 0x1 | 192.12 [186.63] | 13.9 / 49.6 | 32.6 / 40.9 |
| 144 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, WARP | 181.13 [537.92] | 11.7 / 23.0 | 38.2 / 40.7 |
| 60 Hz / 100 % | sidebar | rows 1000, weight 1 | 0.0 [0.0] | 17.5 / 17.6 | 31.1 / 33.5 |
| 60 Hz / 100 % | sidebar | rows 1000000, weight 1 | 0.0 [0.0] | 17.4 / 17.6 | 21.4 / 33.7 |
| 60 Hz / 100 % | sidebar | rows 1000000, weight 2 | 0.0 [0.0] | 17.6 / 18.0 | 21.4 / 34.0 |
| 60 Hz / 100 % | sidebar | rows 1000000, weight 2, affinity 0x1 | 75.59 [0.0] | 29.8 / 31.8 | 31.4 / 34.2 |
| 60 Hz / 100 % | sidebar | rows 1000000, weight 2, WARP | 185.55 [0.0] | 50.3 / 50.6 | 94.5 / 96.8 |
| 60 Hz / 100 % | sidebar | rows 1000000, weight 2, WARP, affinity 0x1 | 192.42 [0.0] | 55.3 / 56.0 | 85.7 / 88.8 |
| 60 Hz / 100 % | pages | rows 10000, weight 1, fresh 0 | 0.0 [0.0] | 17.7 / 18.0 | 30.9 / 34.2 |
| 60 Hz / 100 % | pages | rows 10000, weight 2, fresh 0 | 0.0 [0.0] | 17.9 / 18.3 | 22.9 / 34.1 |
| 60 Hz / 100 % | pages | rows 10000, weight 1, fresh 1 | 0.0 [0.0] | 18.0 / 22.0 | 24.0 / 27.5 |
| 60 Hz / 100 % | pages | rows 10000, weight 2, fresh 1 | 5.49 [0.0] | 19.7 / 31.3 | 39.3 / 50.4 |
| 60 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, uncached art ×4 | 21.96 [0.0] | 22.6 / 30.4 | 46.7 / 50.5 |
| 60 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, affinity 0x1 | 82.34 [0.0] | 28.6 / 44.2 | 45.1 / 49.0 |
| 60 Hz / 100 % | pages | rows 10000, weight 2, fresh 1, WARP | 170.16 [0.0] | 51.3 / 52.5 | 89.0 / 93.0 |
| 144 Hz / 125 % | sidebar | rows 1000000, weight 1 | 34.36 [89.34] | 7.5 / 14.1 | 18.8 / 21.8 |
| 144 Hz / 125 % | sidebar | rows 1000000, weight 2 | 41.23 [75.59] | 7.8 / 14.0 | 18.3 / 21.1 |
| 144 Hz / 125 % | pages | rows 10000, weight 1, fresh 0 | 104.29 [142.71] | 13.8 / 14.2 | 17.9 / 21.3 |
| 144 Hz / 125 % | pages | rows 10000, weight 2, fresh 0 | 65.87 [137.22] | 8.2 / 14.2 | 18.1 / 20.2 |
| 144 Hz / 125 % | pages | rows 10000, weight 1, fresh 1 | 54.89 [54.89] | 8.1 / 15.1 | 18.9 / 21.6 |
| 144 Hz / 125 % | pages | rows 10000, weight 2, fresh 1 | 120.76 [131.74] | 10.1 / 22.2 | 20.6 / 23.3 |
| 60 Hz / 125 % | sidebar | rows 1000000, weight 1 | 0.0 [0.0] | 17.5 / 17.9 | 26.2 / 29.3 |
| 60 Hz / 125 % | sidebar | rows 1000000, weight 2 | 0.0 [0.0] | 17.6 / 17.9 | 31.1 / 34.0 |
| 60 Hz / 125 % | pages | rows 10000, weight 1, fresh 0 | 0.0 [0.0] | 17.6 / 18.2 | 22.4 / 26.1 |
| 60 Hz / 125 % | pages | rows 10000, weight 2, fresh 0 | 0.0 [0.0] | 18.0 / 18.4 | 44.3 / 48.2 |
| 60 Hz / 125 % | pages | rows 10000, weight 1, fresh 1 | 0.0 [0.0] | 18.2 / 21.8 | 22.4 / 33.6 |
| 60 Hz / 125 % | pages | rows 10000, weight 2, fresh 1 | 0.0 [0.0] | 19.4 / 25.0 | 47.5 / 51.0 |
| 144 Hz / 150 % | sidebar | rows 1000000, weight 1 | 34.36 [75.59] | 7.7 / 14.4 | 18.4 / 21.2 |
| 144 Hz / 150 % | sidebar | rows 1000000, weight 2 | 20.62 [27.49] | 7.7 / 14.2 | 19.3 / 22.2 |
| 144 Hz / 150 % | pages | rows 10000, weight 1, fresh 0 | 65.87 [131.74] | 7.8 / 14.1 | 18.3 / 21.4 |
| 144 Hz / 150 % | pages | rows 10000, weight 2, fresh 0 | 76.85 [126.25] | 8.2 / 20.9 | 18.8 / 21.1 |
| 144 Hz / 150 % | pages | rows 10000, weight 1, fresh 1 | 49.4 [82.33] | 7.6 / 14.1 | 18.0 / 21.1 |
| 144 Hz / 150 % | pages | rows 10000, weight 2, fresh 1 | 109.78 [98.8] | 9.4 / 21.1 | 18.4 / 24.0 |
| 60 Hz / 150 % | sidebar | rows 1000000, weight 1 | 0.0 [0.0] | 17.4 / 17.7 | 27.8 / 30.6 |
| 60 Hz / 150 % | sidebar | rows 1000000, weight 2 | 0.0 [0.0] | 17.7 / 17.9 | 28.4 / 31.4 |
| 60 Hz / 150 % | pages | rows 10000, weight 1, fresh 0 | 0.0 [0.0] | 17.7 / 18.2 | 30.2 / 33.5 |
| 60 Hz / 150 % | pages | rows 10000, weight 2, fresh 0 | 0.0 [0.0] | 17.9 / 18.4 | 26.0 / 29.7 |
| 60 Hz / 150 % | pages | rows 10000, weight 1, fresh 1 | 0.0 [0.0] | 18.3 / 21.7 | 29.2 / 32.8 |
| 60 Hz / 150 % | pages | rows 10000, weight 2, fresh 1 | 21.96 [5.49] | 21.3 / 30.5 | 46.8 / 50.5 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000, weight 1 | 0.0 [34.36] | 7.4 / 8.3 | 18.6 / 21.4 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 1 | 0.0 [27.49] | 7.4 / 8.7 | 18.6 / 21.7 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 2 | 0.0 [20.62] | 7.4 / 8.2 | 19.0 / 21.8 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 2, affinity 0x1 | 0.0 [6.87] | 8.2 / 9.6 | 11.4 / 14.8 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 2, WARP | 343.61 [268.01] | 20.7 / 22.4 | 38.1 / 41.3 |
| 144 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 2, WARP, affinity 0x1 | 4405.09 [4253.81] | 39.3 / 48.2 | 79.8 / 88.8 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 1, fresh 0 | 0.0 [32.93] | 7.3 / 8.1 | 18.1 / 21.4 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 0 | 0.0 [5.49] | 7.3 / 8.6 | 18.5 / 21.1 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 1, fresh 1 | 21.96 [27.44] | 7.3 / 11.5 | 11.6 / 14.4 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 1 | 71.36 [27.44] | 7.4 / 20.5 | 21.3 / 24.2 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 1, uncached art ×4 | 71.36 [49.4] | 7.6 / 21.8 | 22.6 / 24.7 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 1, affinity 0x1 | 104.29 [54.89] | 8.1 / 25.4 | 23.0 / 25.3 |
| 144 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 1, WARP | 170.16 [367.76] | 11.5 / 23.0 | 37.7 / 41.0 |
| 60 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 1 | 0.0 [0.0] | 17.2 / 17.4 | 30.8 / 34.1 |
| 60 Hz / 100 % · D3D12 | sidebar | rows 1000000, weight 2 | 0.0 [0.0] | 17.4 / 17.6 | 21.6 / 34.2 |
| 60 Hz / 100 % · D3D12 | pages | rows 10000, weight 1, fresh 0 | 0.0 [0.0] | 17.3 / 17.8 | 29.8 / 33.8 |
| 60 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 0 | 0.0 [0.0] | 17.6 / 18.4 | 30.7 / 34.3 |
| 60 Hz / 100 % · D3D12 | pages | rows 10000, weight 1, fresh 1 | 0.0 [0.0] | 18.0 / 18.3 | 23.1 / 34.5 |
| 60 Hz / 100 % · D3D12 | pages | rows 10000, weight 2, fresh 1 | 5.49 [0.0] | 19.0 / 27.2 | 46.5 / 49.9 |
| 144 Hz / 125 % · D3D12 | sidebar | rows 1000000, weight 1 | 0.0 [20.62] | 7.4 / 9.0 | 11.7 / 14.4 |
| 144 Hz / 125 % · D3D12 | sidebar | rows 1000000, weight 2 | 0.0 [20.62] | 7.5 / 8.0 | 18.8 / 21.6 |
| 144 Hz / 125 % · D3D12 | pages | rows 10000, weight 1, fresh 0 | 0.0 [16.47] | 7.3 / 8.6 | 12.3 / 14.4 |
| 144 Hz / 125 % · D3D12 | pages | rows 10000, weight 2, fresh 0 | 0.0 [5.49] | 7.3 / 9.2 | 19.0 / 21.1 |
| 144 Hz / 125 % · D3D12 | pages | rows 10000, weight 1, fresh 1 | 16.47 [10.98] | 7.4 / 13.3 | 19.0 / 21.8 |
| 144 Hz / 125 % · D3D12 | pages | rows 10000, weight 2, fresh 1 | 71.36 [16.47] | 7.4 / 20.2 | 20.2 / 22.9 |
| 144 Hz / 150 % · D3D12 | sidebar | rows 1000000, weight 1 | 0.0 [0.0] | 7.3 / 7.8 | 17.8 / 21.1 |
| 144 Hz / 150 % · D3D12 | sidebar | rows 1000000, weight 2 | 0.0 [0.0] | 7.6 / 9.7 | 18.7 / 21.5 |
| 144 Hz / 150 % · D3D12 | pages | rows 10000, weight 1, fresh 0 | 0.0 [0.0] | 7.3 / 8.3 | 18.7 / 21.0 |
| 144 Hz / 150 % · D3D12 | pages | rows 10000, weight 2, fresh 0 | 0.0 [0.0] | 7.3 / 8.9 | 10.9 / 14.2 |
| 144 Hz / 150 % · D3D12 | pages | rows 10000, weight 1, fresh 1 | 21.96 [0.0] | 7.3 / 11.6 | 11.6 / 13.9 |
| 144 Hz / 150 % · D3D12 | pages | rows 10000, weight 2, fresh 1 | 60.38 [32.93] | 7.4 / 21.8 | 21.2 / 24.9 |

## Glow

| condition | load | GPU busy p50 / p99 ms | frame p99 / max ms | dropped/min presents [display] |
|---|---|---|---|---|
| 144 Hz / 100 % | rows 10000, weight 1, glow 0 | 0.5 / 5.1 | 7.4 / 8.3 | 0.0 [7.45] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 1 | 0.5 / 5.2 | 7.4 / 14.0 | 14.91 [29.81] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 2 | 0.5 / 5.3 | 7.5 / 14.0 | 14.91 [22.36] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 0, iGPU | 2.1 / 8.3 | 13.9 / 14.1 | 126.69 [134.14] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 1, iGPU | 2.3 / 7.6 | 13.8 / 14.1 | 111.79 [111.79] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 2, iGPU | 2.3 / 7.7 | 13.9 / 14.2 | 126.69 [141.6] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 0, WARP | 0.8 / 1.1 | 21.3 / 21.6 | 1021.0 [74.52] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 1, WARP | 0.8 / 1.1 | 21.0 / 21.8 | 909.21 [14.9] |
| 144 Hz / 100 % | rows 10000, weight 1, glow 2, WARP | 1.7 / 2.2 | 22.1 / 22.5 | 342.82 [14.9] |
| 144 Hz / 100 % | rows 10000, weight 2, glow 2, WARP | 2.0 / 2.5 | 22.2 / 23.1 | 1512.86 [67.07] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 0 | 5.7 / 7.9 | 17.5 / 17.9 | 0.0 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 1 | 5.7 / 7.8 | 17.6 / 18.3 | 0.0 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 2 | 5.8 / 7.5 | 17.4 / 17.9 | 0.0 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 0, iGPU | 7.8 / 10.2 | 17.1 / 18.6 | 0.0 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 1, iGPU | 7.8 / 10.4 | 17.4 / 33.4 | 7.45 [7.45] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 2, iGPU | 7.8 / 9.2 | 17.2 / 17.9 | 0.0 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 0, WARP | 0.9 / 1.2 | 50.5 / 50.9 | 305.55 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 1, WARP | 0.9 / 1.2 | 50.6 / 50.7 | 447.15 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 1, glow 2, WARP | 1.8 / 2.3 | 51.0 / 51.6 | 149.05 [0.0] |
| 60 Hz / 100 % | rows 10000, weight 2, glow 2, WARP | 2.1 / 2.5 | 51.0 / 51.9 | 305.55 [0.0] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 0 | 2.0 / 6.2 | 7.4 / 8.1 | 0.0 [14.9] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 1 | 2.1 / 5.6 | 7.5 / 8.0 | 0.0 [7.45] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 2 | 2.4 / 6.3 | 7.4 / 8.1 | 0.0 [22.36] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 0, iGPU | 2.3 / 8.3 | 13.9 / 14.4 | 156.5 [163.95] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 1, iGPU | 2.3 / 8.2 | 13.9 / 14.6 | 201.22 [201.22] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 2, iGPU | 2.4 / 7.5 | 13.9 / 14.1 | 134.15 [134.14] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 0, WARP | 0.8 / 1.1 | 21.4 / 21.8 | 1021.0 [59.62] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 1, WARP | 0.8 / 1.1 | 21.3 / 21.7 | 752.7 [52.17] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 1, glow 2, WARP | 1.7 / 2.1 | 21.8 / 22.6 | 253.39 [81.98] |
| 144 Hz / 100 % · D3D12 | rows 10000, weight 2, glow 2, WARP | 2.0 / 2.6 | 22.1 / 22.7 | 879.38 [52.17] |

## Text

| scale | renderer | acutance (dark / white band) | solid ink (dark / white) | colour fringing (white band) | line width px (12/13/14) |
|---|---|---|---|---|---|
| 100 % | DirectWrite ClearType | 0.446 / 0.435 | 0.50 / 0.35 | 113 | 147/147/155/155/192/192 |
| 100 % | DirectWrite grayscale | 0.450 / 0.465 | 0.60 / 0.39 | 0 | 146/146/155/155/190/190 |
| 100 % | GDI ClearType | 0.534 / 0.527 | 0.52 / 0.47 | 118 | 148/160/183 |
| 100 % | Qt CurveRendering | 0.495 / 0.495 | 0.42 / 0.42 | 0 | 145/145/159/159/182/182 |
| 100 % | Qt NativeRendering | 0.530 / 0.530 | 0.52 / 0.52 | 114 | 148/148/160/160/183/183 |
| 100 % | Qt QtRendering | 0.417 / 0.416 | 0.33 / 0.33 | 89 | 146/146/159/159/184/184 |
| 125 % | DirectWrite ClearType | 0.445 / 0.436 | 0.53 / 0.46 | 107 | 183/183/194/194/239/239 |
| 125 % | DirectWrite grayscale | 0.429 / 0.459 | 0.62 / 0.47 | 0 | 183/183/193/193/239/239 |
| 125 % | GDI ClearType | 0.432 / 0.430 | 0.47 / 0.41 | 121 | 179/179/189/189/235/235 |
| 125 % | Qt CurveRendering | 0.440 / 0.440 | 0.41 / 0.41 | 0 | 183/183/193/193/230/230 |
| 125 % | Qt NativeRendering | 0.449 / 0.449 | 0.49 / 0.49 | 116 | 183/183/193/193/230/230 |
| 125 % | Qt QtRendering | 0.382 / 0.381 | 0.34 / 0.34 | 89 | 183/183/194/194/230/230 |
| 150 % | DirectWrite ClearType | 0.397 / 0.420 | 0.56 / 0.48 | 94 | 220/220/233/233/287/287 |
| 150 % | DirectWrite grayscale | 0.394 / 0.428 | 0.62 / 0.48 | 0 | 219/219/231/231/286/286 |
| 150 % | GDI ClearType | 0.419 / 0.433 | 0.49 / 0.44 | 100 | 220/220/234/234/275/275 |
| 150 % | Qt CurveRendering | 0.411 / 0.412 | 0.46 / 0.46 | 0 | 219/219/232/232/276/276 |
| 150 % | Qt NativeRendering | 0.437 / 0.438 | 0.52 / 0.52 | 107 | 219/219/232/232/276/276 |
| 150 % | Qt QtRendering | 0.383 / 0.384 | 0.38 / 0.38 | 85 | 219/219/232/232/276/276 |

## Baselines

| condition | app | input | content updates/s while moving | skipped refreshes per min moving | longest freeze ms | PresentMon target | frames/expected, dropped |
|---|---|---|---|---|---|---|---|
| 144 Hz / 100 % | qt | fling:2000 | 142.7 | 66.7 | 13.9 | app | 1179/1180, 1 |
| 144 Hz / 100 % | qt | fling:8000 | 143.1 | 43.8 | 13.5 | app | 1157/1158, 2 |
| 144 Hz / 100 % | qt | wheel:10 | 143.6 | 14.9 | 14.1 | app | 1281/1287, 6 |
| 144 Hz / 100 % | explorer | fling:2000 | 82.3 | 3688.0 | 29.4 | dwm | 794/1180, 349 |
| 144 Hz / 100 % | explorer | fling:8000 | 74.0 | 4209.5 | 30.1 | dwm | 475/1158, 680 |
| 144 Hz / 100 % | explorer | wheel:10 | 10.0 | 8025.5 | 107.8 | dwm | 87/1287, 905 |
| 60 Hz / 100 % | qt | fling:2000 | 60.0 | 0.0 | 17.4 | app | 492/492, 0 |
| 60 Hz / 100 % | qt | fling:8000 | 58.4 | 92.2 | 33.3 | app | 483/483, 0 |
| 60 Hz / 100 % | qt | wheel:10 | 59.9 | 0.0 | 17.2 | app | 537/537, 0 |
| 60 Hz / 100 % | explorer | fling:2000 | 44.1 | 952.9 | 133.6 | dwm | 287/492, 192 |
| 60 Hz / 100 % | explorer | fling:8000 | 35.3 | 1482.7 | 83.6 | dwm | 316/483, 166 |
| 60 Hz / 100 % | explorer | wheel:10 | 10.0 | 2995.3 | 117.6 | dwm | 85/537, 334 |
| 144 Hz / 125 % | qt | fling:2000 | 142.9 | 54.1 | 14.1 | app | 1178/1180, 2 |
| 144 Hz / 125 % | qt | fling:8000 | 142.7 | 69.3 | 14.1 | app | 1159/1158, 0 |
| 144 Hz / 125 % | qt | wheel:10 | 143.6 | 14.9 | 14.1 | app | 1283/1287, 5 |
| 144 Hz / 125 % | explorer | fling:2000 | 59.5 | 5063.8 | 153.6 | dwm | 535/1180, 616 |
| 144 Hz / 125 % | explorer | fling:8000 | 52.2 | 5513.5 | 83.9 | dwm | 606/1158, 552 |
| 144 Hz / 125 % | explorer | wheel:10 | 10.0 | 7864.3 | 104.0 | dwm | 123/1287, 750 |
| 60 Hz / 125 % | qt | fling:2000 | 60.0 | 0.0 | 17.2 | app | 492/492, 0 |
| 60 Hz / 125 % | qt | fling:8000 | 60.2 | 0.0 | 17.0 | app | 483/483, 0 |
| 60 Hz / 125 % | qt | wheel:10 | 59.8 | 7.5 | 33.6 | app | 536/537, 0 |
| 60 Hz / 125 % | explorer | fling:2000 | 41.2 | 1126.6 | 133.8 | dwm | 288/492, 192 |
| 60 Hz / 125 % | explorer | fling:8000 | 40.8 | 1155.8 | 83.6 | dwm | 320/483, 162 |
| 60 Hz / 125 % | explorer | wheel:10 | 10.0 | 2997.5 | 107.1 | dwm | 127/537, 252 |
| 144 Hz / 150 % | qt | fling:2000 | 142.4 | 92.9 | 13.9 | app | 1179/1180, 1 |
| 144 Hz / 150 % | qt | fling:8000 | 143.6 | 0.0 | 7.4 | app | 1156/1158, 2 |
| 144 Hz / 150 % | qt | wheel:10 | 143.2 | 37.1 | 14.3 | app | 1284/1287, 4 |
| 144 Hz / 150 % | explorer | fling:2000 | 60.7 | 4986.0 | 167.7 | dwm | 483/1180, 660 |
| 144 Hz / 150 % | explorer | fling:8000 | 49.7 | 5627.2 | 91.2 | dwm | 583/1158, 583 |
| 144 Hz / 150 % | explorer | wheel:10 | 10.0 | 7891.5 | 105.1 | dwm | 134/1287, 830 |
| 60 Hz / 150 % | qt | fling:2000 | 60.0 | 0.0 | 17.3 | app | 492/492, 0 |
| 60 Hz / 150 % | qt | fling:8000 | 60.3 | 0.0 | 16.9 | app | 482/483, 0 |
| 60 Hz / 150 % | qt | wheel:10 | 59.9 | 0.0 | 17.2 | app | 536/536, 0 |
| 60 Hz / 150 % | explorer | fling:2000 | 40.5 | 1164.9 | 167.2 | dwm | 288/492, 204 |
| 60 Hz / 150 % | explorer | fling:8000 | 40.1 | 1185.9 | 99.9 | dwm | 323/483, 159 |
| 60 Hz / 150 % | explorer | wheel:10 | 10.0 | 2999.8 | 101.9 | dwm | 137/537, 272 |
| 144 Hz / 100 % · D3D12 | qt | fling:2000 | 142.9 | 61.7 | 14.0 | app | 1180/1180, 1 |
| 144 Hz / 100 % · D3D12 | qt | fling:8000 | 143.3 | 35.5 | 13.8 | app | 1158/1158, 0 |
| 144 Hz / 100 % · D3D12 | qt | wheel:10 | 143.9 | 0.0 | 7.4 | app | 1288/1287, 0 |
| 144 Hz / 100 % · D3D12 | explorer | fling:2000 | 61.7 | 4923.0 | 118.8 | dwm | 449/1180, 698 |
| 144 Hz / 100 % · D3D12 | explorer | fling:8000 | 51.9 | 5499.0 | 76.9 | dwm | 496/1158, 655 |
| 144 Hz / 100 % · D3D12 | explorer | wheel:10 | 10.0 | 8018.3 | 109.0 | dwm | 86/1287, 917 |

## Startup

144 Hz / 100 %: first present after launch, ms: 543.3, 525.7, 533.2, 515.9, 530.2
60 Hz / 100 %: first present after launch, ms: 541.6, 549.8, 551.5, 530.9, 564.4
144 Hz / 100 % · D3D12: first present after launch, ms: 587.5, 597.5, 585.6, 607.4, 599.7

## Idle memory

| condition | config | private MB | working set MB | CPU % |
|---|---|---|---|---|
| c59s100 | heartbeat on, d3d11 | 151.9 | 149.5 | 0.39 |
| c59s100 | heartbeat off, d3d11 | 147.5 | 148.6 | 0.00 |
| c144s100 | heartbeat on, d3d11 | 149.8 | 150.6 | 0.00 |
| c144s100 | heartbeat off, d3d11 | 151.0 | 149.8 | 0.00 |
| d12c144s100 | heartbeat on, d3d11 | 311.7 | 280.2 | 0.00 |
| d12c144s100 | heartbeat off, d3d11 | 310.5 | 279.1 | 0.00 |

## Soak

**144 Hz / 100 % (D3D11)**: 257333 presents in 30 min; dropped frames per minute: [24, 24, 19, 24, 25, 27, 22, 32, 23, 28, 23, 12, 20, 24, 25, 26, 27, 23, 23, 24, 26, 30, 24, 26, 25, 30, 24, 22, 24, 28]. First half 352, second half 382. Display drops per minute (MsBetweenDisplayChange): [37, 36, 32, 39, 37, 39, 26, 40, 35, 41, 35, 24, 30, 36, 40, 45, 38, 30, 33, 30, 34, 44, 32, 31, 38, 45, 35, 35, 36, 46], first half 527, second half 552. Private memory after warm-up: 261.0 MB (minutes 5–10) → 264.2 MB (last 5 min), slope +0.177 MB/min, min 259.0, max 265.3 MB.

**144 Hz / 100 % (D3D12)**: 258054 presents in 30 min; dropped frames per minute: [3, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]. First half 7, second half 6. Display drops per minute (MsBetweenDisplayChange): [26, 19, 30, 15, 23, 27, 14, 18, 30, 15, 19, 13, 18, 20, 33, 18, 20, 25, 17, 24, 17, 25, 14, 22, 29, 20, 14, 22, 22, 20], first half 320, second half 309. Private memory after warm-up: 425.6 MB (minutes 5–10) → 434.5 MB (last 5 min), slope +0.482 MB/min, min 424.1, max 437.0 MB.

**144 Hz / 100 % (D3D12)**: 516978 presents in 60 min; dropped frames per minute: [3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 1, 2, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 3, 1, 2, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0]. First half 16, second half 9. Display drops per minute (MsBetweenDisplayChange): [22, 25, 12, 24, 26, 20, 24, 15, 14, 16, 16, 28, 27, 25, 31, 25, 25, 27, 18, 31, 29, 30, 18, 19, 20, 34, 16, 12, 23, 27, 38, 23, 25, 20, 32, 26, 22, 19, 28, 15, 19, 36, 21, 14, 32, 14, 18, 16, 22, 26, 20, 36, 33, 19, 16, 25, 22, 16, 24, 22], first half 679, second half 699. Private memory after warm-up: 427.2 MB (minutes 5–10) → 437.2 MB (last 5 min), slope +0.158 MB/min, min 425.6, max 437.5 MB.
