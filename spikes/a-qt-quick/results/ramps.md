# Spike A ramp tables (generated)

#### 144 Hz / 100 % · artwork

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.4 | 8.3 | 0.0 | 0.5/4.3 | 0.8/1.0 |
| 1 | rows 10000, weight 1, uncached art ×1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 14.0 | 7.45 | 0.5/5.1 | 7.0/0.8 |
| 2 | rows 10000, weight 1, uncached art ×4, fling:8000 | 1157/1158 | 7.0 | 7.5 | 14.1 | 7.45 | 0.5/5.2 | 7.1/1.1 |
| 3 | rows 10000, weight 1, uncached art ×16, fling:8000 | 1158/1158 | 6.9 | 7.6 | 8.6 | 0.0 | 0.5/5.4 | 1.1/1.1 |

#### 144 Hz / 100 % · churn

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, churn/s 1, fling:8000 | 1158/1158 | 6.9 | 7.3 | 8.2 | 0.0 | 0.5/5.0 | 0.8/0.6 |
| 1 | rows 10000, weight 1, churn/s 10, fling:8000 | 1157/1158 | 6.9 | 8.0 | 14.0 | 7.45 | 0.6/5.4 | 7.0/0.5 |
| 2 | rows 10000, weight 1, churn/s 100 (ran 125), fling:8000 | 1156/1158 | 6.9 | 8.0 | 14.3 | 14.91 | 0.9/5.4 | 7.2/0.3 |
| 3 | rows 10000, weight 1, churn/s 1000, fling:8000 | 1157/1158 | 6.8 | 9.2 | 13.8 | 14.91 | 0.8/5.9 | 8.1/0.3 |

#### 144 Hz / 100 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1156/1156 | 6.9 | 8.2 | 10.3 | 0.0 | 0.9/5.8 | 1.6/1.6 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1002/1156 | 7.0 | 39.2 | 40.5 | 1471.19 | 1.2/6.1 | 28.8/2.3 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1157/1156 | 6.6 | 21.1 | 21.5 | 2658.61 | 1.2/1.6 | 12.1/1.2 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 486/1156 | 14.4 | 54.0 | 82.3 | 5279.86 | 13.4/42.9 | 59.4/11.5 |

#### 144 Hz / 100 % · cpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1157/1158 | 7.0 | 7.4 | 14.0 | 7.45 | 0.5/5.4 | 7.0/1.0 |
| 1 | rows 10000, weight 1, affinity 0x5, fling:8000 | 1157/1158 | 7.0 | 7.5 | 14.0 | 7.45 | 0.6/5.5 | 7.1/1.8 |
| 2 | rows 10000, weight 1, affinity 0x1, fling:8000 | 1157/1158 | 7.0 | 7.7 | 13.8 | 74.53 | 0.7/5.0 | 6.9/6.1 |
| 3 | rows 10000, weight 1, power saver, fling:8000 | 1158/1158 | 7.0 | 7.6 | 13.8 | 7.45 | 0.5/5.2 | 6.9/1.6 |
| 4 | rows 10000, weight 1, affinity 0x1, power saver, fling:8000 | 1157/1158 | 6.9 | 11.7 | 14.2 | 96.88 | 0.7/6.2 | 7.0/6.7 |

#### 144 Hz / 100 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1208/1209 | 7.0 | 7.6 | 8.0 | 0.0 | 0.6/5.1 | 0.9/0.9 |
| 1 | rows 10000, weight 1, fling:2000 | 1180/1180 | 7.0 | 7.5 | 7.8 | 0.0 | 0.5/5.1 | 0.6/1.1 |
| 2 | rows 10000, weight 1, fling:4000 | 1164/1165 | 7.0 | 7.5 | 14.1 | 7.41 | 0.6/5.4 | 7.2/1.1 |
| 3 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.3 | 0.0 | 0.5/4.8 | 0.9/1.2 |
| 4 | rows 10000, weight 1, fling:12000 | 1154/1156 | 6.9 | 7.5 | 14.0 | 14.94 | 0.6/5.3 | 7.1/0.9 |

#### 144 Hz / 100 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.4 | 8.1 | 0.0 | 0.5/4.4 | 0.7/0.9 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1144/1158 | 6.9 | 13.8 | 14.1 | 104.34 | 2.3/7.8 | 8.1/1.8 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1159/1158 | 6.9 | 21.1 | 21.8 | 357.72 | 0.9/1.2 | 10.1/1.7 |

#### 144 Hz / 100 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 143.4 | 1 (28.7) | 13.6 | 0 | 7.6 |
| 1 | rows 1000, weight 1, fling:8000 | 143.7 | 0 (0.0) | 8.2 | 2 | 13.9 |
| 2 | rows 1000, weight 1, wheel:10 | 143.7 | 1 (7.4) | 13.9 | 1 | 13.9 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.5 | 3 (22.0) | 14.0 | 1 | 13.7 |

#### 144 Hz / 100 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 124.6 | 35 (1148.0) | 23.1 | 667 | 26.7 |
| 1 | rows 1000, weight 1, fling:8000 | 86.3 | 49 (3430.5) | 23.0 | 654 | 26.8 |
| 2 | rows 1000, weight 1, wheel:10 | 62.5 | 654 (4865.6) | 24.8 | 725 | 24.8 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.5 | 663 (4863.5) | 25.7 | 733 | 25.7 |

#### 144 Hz / 100 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.6 | 8.1 | 0.0 | 0.6/5.2 | 0.9/0.9 |
| 1 | rows 10000, weight 1, fling:8000 | 1157/1158 | 7.0 | 7.5 | 13.9 | 7.45 | 0.5/5.5 | 7.0/1.0 |
| 2 | rows 100000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.2 | 0.0 | 0.6/4.9 | 1.1/1.1 |
| 3 | rows 1000000, weight 1, fling:8000 | 1154/1158 | 6.9 | 7.4 | 14.0 | 29.81 | 0.5/5.7 | 7.0/1.1 |

#### 144 Hz / 100 % · touchpad

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, touchpad:1000 | 1297/1301 | 7.0 | 7.4 | 13.9 | 26.55 | 0.5/6.2 | 7.1/1.0 |
| 1 | rows 10000, weight 1, touchpad:4000 | 1294/1301 | 6.9 | 7.6 | 14.1 | 39.82 | 0.6/6.5 | 7.1/1.4 |
| 2 | rows 10000, weight 1, touchpad:10000 | 1297/1301 | 6.9 | 7.5 | 14.1 | 19.91 | 0.6/5.7 | 6.9/1.3 |
| 3 | rows 10000, weight 1, touchpad:20000 | 1297/1301 | 6.9 | 7.6 | 14.1 | 26.55 | 0.6/5.5 | 7.1/0.8 |

#### 144 Hz / 100 % · warp_production

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, WARP, fling:2000 | 1180/1180 | 6.9 | 20.7 | 21.7 | 438.97 | 0.9/1.1 | 10.9/1.1 |
| 1 | rows 1000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.4 | 21.9 | 1102.97 | 0.8/1.1 | 10.6/1.2 |
| 2 | rows 1000, weight 1, WARP, wheel:10 | 1287/1287 | 6.9 | 21.4 | 22.3 | 1595.72 | 0.9/1.1 | 9.8/1.1 |
| 3 | rows 1000, weight 1, WARP, touchpad:4000 | 1300/1301 | 6.9 | 21.5 | 21.9 | 1028.64 | 0.9/1.2 | 10.0/1.7 |

#### 144 Hz / 100 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1158/1158 | 7.0 | 7.4 | 7.8 | 0.0 | 0.5/4.9 | 0.5/1.2 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.2 | 0.0 | 0.5/4.9 | 0.8/1.2 |
| 2 | rows 10000, weight 3, fling:8000 | 1157/1158 | 6.9 | 7.5 | 13.9 | 7.45 | 0.8/5.6 | 6.9/1.5 |
| 3 | rows 10000, weight 2, fling:8000 | 1158/1158 | 6.9 | 7.8 | 8.4 | 0.0 | 0.8/5.5 | 0.6/2.0 |

#### 144 Hz / 100 % · wheel

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, wheel:5 | 1268/1273 | 6.9 | 7.5 | 14.0 | 33.9 | 0.7/6.4 | 7.1/0.9 |
| 1 | rows 10000, weight 1, wheel:10 | 1285/1287 | 6.9 | 7.5 | 14.1 | 20.11 | 0.7/6.4 | 7.1/1.4 |
| 2 | rows 10000, weight 1, wheel:20 | 1291/1295 | 7.0 | 7.5 | 13.8 | 26.67 | 0.6/6.1 | 7.1/1.0 |
| 3 | rows 10000, weight 1, wheel:40 | 1295/1298 | 7.0 | 7.5 | 13.9 | 19.95 | 0.8/6.3 | 7.1/0.9 |

#### 60 Hz / 100 % · artwork

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.7 | 18.1 | 0.0 | 6.5/8.2 | 1.0/0.9 |
| 1 | rows 10000, weight 1, uncached art ×1, fling:8000 | 483/483 | 16.7 | 17.7 | 18.0 | 0.0 | 6.4/7.7 | 1.1/1.0 |
| 2 | rows 10000, weight 1, uncached art ×4, fling:8000 | 483/483 | 16.7 | 17.9 | 18.1 | 0.0 | 6.4/7.9 | 0.9/1.3 |
| 3 | rows 10000, weight 1, uncached art ×16, fling:8000 | 483/483 | 16.7 | 17.7 | 19.7 | 0.0 | 5.7/7.4 | 0.6/3.2 |

#### 60 Hz / 100 % · churn

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, churn/s 1, fling:8000 | 483/483 | 16.7 | 17.6 | 17.9 | 0.0 | 6.0/7.7 | 0.6/0.8 |
| 1 | rows 10000, weight 1, churn/s 10, fling:8000 | 482/483 | 16.7 | 17.8 | 18.3 | 0.0 | 5.6/6.6 | 1.1/0.7 |
| 2 | rows 10000, weight 1, churn/s 100 (ran 125), fling:8000 | 483/483 | 16.7 | 17.7 | 18.5 | 0.0 | 5.1/6.3 | 1.8/0.3 |
| 3 | rows 10000, weight 1, churn/s 1000, fling:8000 | 482/483 | 16.7 | 17.8 | 18.2 | 0.0 | 6.5/7.7 | 1.3/0.4 |

#### 60 Hz / 100 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 482/482 | 16.7 | 18.0 | 21.1 | 0.0 | 6.6/7.1 | 1.9/2.3 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 393/482 | 16.7 | 51.4 | 70.6 | 1179.94 | 5.3/9.2 | 33.0/2.7 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 481/482 | 16.5 | 49.9 | 50.3 | 1150.07 | 1.2/1.7 | 20.6/0.8 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 462/482 | 16.6 | 54.2 | 62.4 | 418.21 | 13.2/19.3 | 28.8/11.0 |

#### 60 Hz / 100 % · cpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.7 | 17.9 | 0.0 | 5.7/7.1 | 0.7/1.4 |
| 1 | rows 10000, weight 1, affinity 0x5, fling:8000 | 482/483 | 16.7 | 17.8 | 20.9 | 0.0 | 5.7/7.3 | 1.0/4.0 |
| 2 | rows 10000, weight 1, affinity 0x1, fling:8000 | 483/483 | 16.7 | 22.4 | 24.3 | 0.0 | 5.6/7.4 | 1.0/6.3 |
| 3 | rows 10000, weight 1, power saver, fling:8000 | 483/483 | 16.7 | 17.6 | 18.1 | 0.0 | 5.7/7.6 | 0.8/1.2 |
| 4 | rows 10000, weight 1, affinity 0x1, power saver, fling:8000 | 483/483 | 16.7 | 22.0 | 23.7 | 0.0 | 6.5/8.2 | 1.2/5.6 |

#### 60 Hz / 100 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 503/504 | 16.7 | 17.4 | 17.8 | 0.0 | 5.2/7.4 | 0.7/1.2 |
| 1 | rows 10000, weight 1, fling:2000 | 492/492 | 16.7 | 17.3 | 18.2 | 0.0 | 5.7/7.5 | 1.1/0.9 |
| 2 | rows 10000, weight 1, fling:4000 | 486/486 | 16.7 | 17.5 | 18.3 | 0.0 | 5.6/7.6 | 0.8/1.1 |
| 3 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.8 | 18.4 | 0.0 | 5.6/7.8 | 1.2/1.0 |
| 4 | rows 10000, weight 1, fling:12000 | 482/482 | 16.7 | 17.2 | 17.8 | 0.0 | 5.8/7.3 | 0.4/1.0 |

#### 60 Hz / 100 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 18.6 | 0.0 | 5.7/7.4 | 1.2/0.8 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 482/483 | 16.7 | 17.2 | 18.4 | 0.0 | 7.8/9.1 | 4.4/1.7 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 482/483 | 16.7 | 50.5 | 50.8 | 365.17 | 0.9/1.1 | 22.3/1.3 |

#### 60 Hz / 100 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 59.9 | 0 (0.0) | 17.7 | 0 | 17.7 |
| 1 | rows 1000, weight 1, fling:8000 | 60.0 | 0 (0.0) | 17.1 | 0 | 17.2 |
| 2 | rows 1000, weight 1, wheel:10 | 59.8 | 1 (7.5) | 33.3 | 0 | 17.2 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.8 | 1 (7.4) | 33.4 | 0 | 17.2 |

#### 60 Hz / 100 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 59.9 | 0 (0.0) | 17.4 | 6 | 33.5 |
| 1 | rows 1000, weight 1, fling:8000 | 59.2 | 1 (46.1) | 33.1 | 10 | 33.6 |
| 2 | rows 1000, weight 1, wheel:10 | 59.3 | 5 (37.2) | 36.5 | 12 | 36.5 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.6 | 3 (22.1) | 33.5 | 5 | 33.5 |

#### 60 Hz / 100 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 482/483 | 16.7 | 17.6 | 18.2 | 0.0 | 5.5/7.3 | 1.1/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.7 | 19.3 | 0.0 | 5.5/6.5 | 1.1/2.9 |
| 2 | rows 100000, weight 1, fling:8000 | 483/483 | 16.7 | 17.7 | 18.0 | 0.0 | 5.8/7.7 | 1.3/1.0 |
| 3 | rows 1000000, weight 1, fling:8000 | 483/483 | 16.7 | 17.5 | 18.0 | 0.0 | 5.6/7.6 | 0.8/1.1 |

#### 60 Hz / 100 % · touchpad

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, touchpad:1000 | 542/542 | 16.7 | 17.3 | 17.7 | 0.0 | 6.7/8.8 | 0.5/0.8 |
| 1 | rows 10000, weight 1, touchpad:4000 | 542/542 | 16.7 | 17.4 | 19.3 | 0.0 | 6.8/9.1 | 0.5/3.2 |
| 2 | rows 10000, weight 1, touchpad:10000 | 542/542 | 16.7 | 17.3 | 17.6 | 0.0 | 6.5/9.1 | 0.5/1.2 |
| 3 | rows 10000, weight 1, touchpad:20000 | 542/542 | 16.7 | 17.3 | 17.7 | 0.0 | 6.5/9.1 | 0.6/1.2 |

#### 60 Hz / 100 % · warp_production

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, WARP, fling:2000 | 492/492 | 16.7 | 49.6 | 50.6 | 139.01 | 0.9/1.2 | 23.1/0.9 |
| 1 | rows 1000, weight 1, WARP, fling:8000 | 483/483 | 16.7 | 50.3 | 50.7 | 417.34 | 0.9/1.1 | 22.8/1.2 |
| 2 | rows 1000, weight 1, WARP, wheel:10 | 536/537 | 16.4 | 50.7 | 50.8 | 1293.96 | 1.0/1.2 | 19.8/0.9 |
| 3 | rows 1000, weight 1, WARP, touchpad:4000 | 542/542 | 16.6 | 50.7 | 50.9 | 550.83 | 0.9/1.2 | 19.4/1.3 |

#### 60 Hz / 100 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 482/483 | 16.7 | 17.5 | 17.8 | 0.0 | 5.7/6.5 | 1.1/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 18.2 | 0.0 | 6.2/8.2 | 0.9/1.4 |
| 2 | rows 10000, weight 3, fling:8000 | 482/483 | 16.7 | 17.7 | 18.2 | 0.0 | 6.1/8.8 | 1.3/1.1 |
| 3 | rows 10000, weight 2, fling:8000 | 482/483 | 16.7 | 18.2 | 19.8 | 0.0 | 6.6/8.9 | 1.1/3.5 |

#### 60 Hz / 100 % · wheel

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, wheel:5 | 531/531 | 16.7 | 17.5 | 17.6 | 0.0 | 6.4/8.4 | 0.8/1.0 |
| 1 | rows 10000, weight 1, wheel:10 | 536/537 | 16.7 | 17.3 | 17.7 | 0.0 | 6.6/8.3 | 0.9/1.0 |
| 2 | rows 10000, weight 1, wheel:20 | 540/540 | 16.7 | 17.3 | 17.5 | 0.0 | 6.5/7.6 | 0.4/0.8 |
| 3 | rows 10000, weight 1, wheel:40 | 541/541 | 16.7 | 17.5 | 19.8 | 0.0 | 6.4/8.9 | 0.6/3.3 |

#### 144 Hz / 125 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1152/1156 | 7.0 | 8.5 | 14.5 | 22.4 | 1.2/5.7 | 7.2/1.3 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1025/1156 | 7.0 | 38.2 | 40.7 | 1254.62 | 1.1/6.9 | 28.2/3.0 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1158/1156 | 6.6 | 21.3 | 21.8 | 2569.02 | 1.5/2.1 | 11.7/1.3 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 365/1155 | 20.0 | 87.3 | 98.3 | 6111.16 | 17.0/49.9 | 68.9/16.1 |

#### 144 Hz / 125 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1208/1209 | 7.0 | 7.7 | 14.0 | 7.14 | 0.7/5.5 | 7.1/0.9 |
| 1 | rows 10000, weight 1, fling:2000 | 1179/1180 | 7.0 | 7.6 | 7.9 | 0.0 | 0.7/5.3 | 0.7/1.1 |
| 2 | rows 10000, weight 1, fling:4000 | 1166/1165 | 6.9 | 7.4 | 8.3 | 0.0 | 0.6/5.0 | 1.1/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.6 | 8.1 | 0.0 | 0.7/5.2 | 0.8/1.0 |
| 4 | rows 10000, weight 1, fling:12000 | 1156/1156 | 7.0 | 7.6 | 8.8 | 0.0 | 0.7/5.2 | 1.1/1.0 |

#### 144 Hz / 125 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1157/1158 | 7.0 | 7.4 | 13.9 | 14.91 | 0.7/5.7 | 6.9/1.0 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1157/1158 | 7.0 | 7.4 | 14.1 | 14.91 | 2.4/6.4 | 10.0/1.8 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.3 | 22.0 | 506.77 | 1.1/1.4 | 11.1/2.0 |

#### 144 Hz / 125 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 142.3 | 3 (95.9) | 14.1 | 2 | 14.1 |
| 1 | rows 1000, weight 1, fling:8000 | 143.9 | 0 (0.0) | 7.5 | 4 | 14.2 |
| 2 | rows 1000, weight 1, wheel:10 | 143.2 | 5 (37.1) | 14.3 | 4 | 14.3 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.5 | 3 (22.0) | 14.2 | 2 | 14.2 |

#### 144 Hz / 125 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 124.1 | 35 (1173.8) | 21.6 | 669 | 22.4 |
| 1 | rows 1000, weight 1, fling:8000 | 93.1 | 35 (3104.7) | 21.4 | 655 | 25.0 |
| 2 | rows 1000, weight 1, wheel:10 | 61.9 | 661 (4910.0) | 34.8 | 730 | 34.8 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.3 | 664 (4884.9) | 34.8 | 734 | 25.5 |

#### 144 Hz / 125 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1157/1158 | 7.0 | 7.5 | 14.0 | 7.45 | 0.6/5.3 | 7.0/0.9 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.5 | 7.9 | 0.0 | 0.7/5.2 | 0.9/1.0 |
| 2 | rows 100000, weight 1, fling:8000 | 1157/1158 | 6.9 | 7.5 | 14.0 | 7.45 | 0.7/5.4 | 6.8/0.8 |
| 3 | rows 1000000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.0 | 0.0 | 0.6/5.2 | 0.9/0.8 |

#### 144 Hz / 125 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1156/1158 | 7.0 | 7.5 | 14.0 | 14.91 | 0.6/5.4 | 7.0/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 1157/1158 | 6.9 | 7.5 | 14.1 | 7.45 | 0.7/5.6 | 7.0/1.1 |
| 2 | rows 10000, weight 3, fling:8000 | 1154/1158 | 6.9 | 7.7 | 14.1 | 29.81 | 1.1/6.1 | 7.1/1.3 |
| 3 | rows 10000, weight 2, fling:8000 | 1158/1158 | 7.0 | 7.7 | 9.8 | 0.0 | 0.7/5.2 | 0.9/2.4 |

#### 60 Hz / 125 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 482/482 | 16.6 | 18.1 | 18.8 | 0.0 | 4.7/6.9 | 1.3/0.7 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 415/482 | 16.6 | 51.6 | 52.8 | 1008.18 | 6.8/8.0 | 33.6/3.4 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 482/482 | 16.6 | 49.7 | 50.6 | 858.82 | 1.7/2.2 | 22.4/1.4 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 304/482 | 24.4 | 104.2 | 125.7 | 1807.25 | 17.2/39.2 | 71.3/6.7 |

#### 60 Hz / 125 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 504/504 | 16.7 | 17.4 | 17.7 | 0.0 | 5.9/8.0 | 0.6/1.2 |
| 1 | rows 10000, weight 1, fling:2000 | 492/492 | 16.7 | 17.4 | 18.1 | 0.0 | 5.8/7.8 | 1.0/1.0 |
| 2 | rows 10000, weight 1, fling:4000 | 486/486 | 16.7 | 17.6 | 17.9 | 0.0 | 5.4/7.7 | 0.6/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.6 | 18.5 | 0.0 | 6.0/7.0 | 0.8/2.3 |
| 4 | rows 10000, weight 1, fling:12000 | 481/482 | 16.7 | 17.3 | 17.6 | 0.0 | 5.9/8.4 | 0.8/1.0 |

#### 60 Hz / 125 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.3 | 19.5 | 0.0 | 6.0/7.9 | 2.1/0.8 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 483/483 | 16.7 | 17.1 | 18.7 | 0.0 | 8.4/10.5 | 5.6/1.8 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 482/483 | 16.7 | 50.7 | 50.8 | 327.91 | 1.2/1.5 | 22.9/1.1 |

#### 60 Hz / 125 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 59.1 | 2 (52.5) | 33.2 | 0 | 17.3 |
| 1 | rows 1000, weight 1, fling:8000 | 61.0 | 0 (0.0) | 16.5 | 0 | 17.2 |
| 2 | rows 1000, weight 1, wheel:10 | 59.9 | 0 (0.0) | 17.5 | 0 | 17.5 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.9 | 0 (0.0) | 17.3 | 0 | 17.3 |

#### 60 Hz / 125 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 60.0 | 0 (0.0) | 17.4 | 7 | 49.9 |
| 1 | rows 1000, weight 1, fling:8000 | 60.0 | 0 (0.0) | 17.3 | 5 | 33.5 |
| 2 | rows 1000, weight 1, wheel:10 | 59.7 | 2 (14.9) | 33.5 | 7 | 33.5 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.5 | 4 (29.4) | 33.4 | 6 | 33.7 |

#### 60 Hz / 125 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 18.0 | 0.0 | 5.9/6.9 | 0.8/0.7 |
| 1 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.6 | 17.9 | 0.0 | 5.9/7.7 | 0.6/0.7 |
| 2 | rows 100000, weight 1, fling:8000 | 483/483 | 16.7 | 17.5 | 18.1 | 0.0 | 6.0/8.5 | 0.7/0.7 |
| 3 | rows 1000000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 18.4 | 0.0 | 6.0/7.9 | 0.9/1.0 |

#### 60 Hz / 125 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 483/483 | 16.7 | 17.6 | 18.3 | 0.0 | 5.9/7.8 | 0.5/2.2 |
| 1 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.6 | 19.2 | 0.0 | 5.8/8.1 | 0.7/2.9 |
| 2 | rows 10000, weight 3, fling:8000 | 483/483 | 16.7 | 17.8 | 18.0 | 0.0 | 5.4/7.7 | 0.5/1.0 |
| 3 | rows 10000, weight 2, fling:8000 | 482/483 | 16.7 | 18.0 | 18.6 | 0.0 | 6.8/10.0 | 0.6/2.4 |

#### 144 Hz / 150 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1153/1156 | 6.9 | 8.6 | 14.1 | 14.94 | 1.4/5.2 | 7.0/1.3 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1028/1156 | 7.0 | 39.9 | 42.1 | 1254.62 | 1.1/6.6 | 34.1/3.4 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1157/1156 | 6.6 | 21.3 | 21.7 | 2576.45 | 2.1/2.9 | 14.2/2.0 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 306/1156 | 20.9 | 109.2 | 129.5 | 6429.94 | 20.2/61.0 | 68.4/39.5 |

#### 144 Hz / 150 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1208/1209 | 6.9 | 7.7 | 8.1 | 0.0 | 0.8/4.9 | 0.7/1.0 |
| 1 | rows 10000, weight 1, fling:2000 | 1179/1180 | 7.0 | 7.7 | 14.1 | 7.32 | 0.8/5.4 | 7.0/1.0 |
| 2 | rows 10000, weight 1, fling:4000 | 1165/1165 | 7.0 | 7.5 | 14.1 | 7.41 | 0.7/5.6 | 6.8/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 1154/1158 | 6.9 | 7.5 | 14.0 | 29.81 | 0.7/6.0 | 7.1/1.1 |
| 4 | rows 10000, weight 1, fling:12000 | 1154/1156 | 6.9 | 7.5 | 14.5 | 7.47 | 0.7/5.1 | 6.9/1.0 |

#### 144 Hz / 150 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1155/1158 | 7.0 | 7.6 | 13.9 | 22.36 | 0.8/5.9 | 7.1/1.0 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1156/1158 | 6.9 | 7.4 | 14.2 | 14.91 | 3.0/5.8 | 7.3/1.9 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.6 | 22.4 | 1184.95 | 1.6/2.1 | 10.9/1.4 |

#### 144 Hz / 150 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 143.4 | 1 (24.9) | 13.6 | 1 | 14.1 |
| 1 | rows 1000, weight 1, fling:8000 | 141.6 | 4 (138.2) | 14.1 | 2 | 14.1 |
| 2 | rows 1000, weight 1, wheel:10 | 143.2 | 5 (37.1) | 14.2 | 5 | 14.2 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.5 | 3 (22.0) | 14.2 | 1 | 14.2 |

#### 144 Hz / 150 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 100.4 | 103 (2595.8) | 21.4 | 668 | 27.7 |
| 1 | rows 1000, weight 1, fling:8000 | 79.1 | 88 (3864.9) | 22.3 | 653 | 28.0 |
| 2 | rows 1000, weight 1, wheel:10 | 62.3 | 652 (4842.4) | 35.3 | 722 | 37.6 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.0 | 663 (4874.4) | 37.4 | 734 | 37.4 |

#### 144 Hz / 150 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.5 | 8.4 | 0.0 | 0.6/5.0 | 0.8/0.9 |
| 1 | rows 10000, weight 1, fling:8000 | 1156/1158 | 7.0 | 7.6 | 14.0 | 14.91 | 0.7/5.0 | 7.0/1.0 |
| 2 | rows 100000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.3 | 0.0 | 0.8/5.6 | 1.0/1.1 |
| 3 | rows 1000000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 14.0 | 7.45 | 0.7/5.2 | 7.0/1.1 |

#### 144 Hz / 150 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1157/1158 | 7.0 | 7.4 | 14.3 | 7.45 | 0.6/5.8 | 6.8/1.1 |
| 1 | rows 10000, weight 1, fling:8000 | 1157/1158 | 7.0 | 7.5 | 13.9 | 7.45 | 0.7/5.7 | 6.9/0.9 |
| 2 | rows 10000, weight 3, fling:8000 | 1157/1158 | 6.9 | 7.8 | 14.1 | 7.45 | 0.8/5.7 | 7.0/1.2 |
| 3 | rows 10000, weight 2, fling:8000 | 1156/1158 | 6.9 | 8.0 | 14.9 | 14.91 | 0.9/5.9 | 7.0/2.0 |

#### 60 Hz / 150 % · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 481/482 | 16.6 | 18.4 | 19.0 | 0.0 | 7.0/8.5 | 1.6/2.2 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 416/482 | 16.8 | 36.1 | 39.3 | 515.29 | 5.2/8.5 | 34.3/2.1 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 482/482 | 16.6 | 50.0 | 50.3 | 940.97 | 2.1/2.8 | 21.8/1.4 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 296/482 | 23.2 | 94.8 | 127.6 | 1814.09 | 18.8/46.1 | 81.2/7.3 |

#### 60 Hz / 150 % · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 504/504 | 16.7 | 17.4 | 18.1 | 0.0 | 6.1/8.3 | 0.8/0.9 |
| 1 | rows 10000, weight 1, fling:2000 | 492/492 | 16.7 | 17.4 | 18.7 | 0.0 | 6.2/8.2 | 0.6/2.5 |
| 2 | rows 10000, weight 1, fling:4000 | 485/486 | 16.7 | 17.5 | 18.0 | 0.0 | 6.8/8.1 | 0.8/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.6 | 18.1 | 0.0 | 6.1/8.3 | 0.7/1.1 |
| 4 | rows 10000, weight 1, fling:12000 | 481/482 | 16.7 | 17.3 | 17.7 | 0.0 | 6.3/9.4 | 0.8/1.0 |

#### 60 Hz / 150 % · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.4 | 18.4 | 0.0 | 6.0/8.9 | 0.9/1.4 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 483/483 | 16.7 | 17.2 | 18.4 | 0.0 | 8.9/11.5 | 4.7/1.6 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 483/483 | 16.7 | 50.9 | 51.3 | 469.51 | 1.7/2.0 | 22.6/1.4 |

#### 60 Hz / 150 % · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 59.4 | 1 (31.8) | 32.8 | 0 | 17.3 |
| 1 | rows 1000, weight 1, fling:8000 | 60.4 | 0 (0.0) | 16.9 | 0 | 17.5 |
| 2 | rows 1000, weight 1, wheel:10 | 59.7 | 2 (14.9) | 33.0 | 0 | 17.3 |
| 3 | rows 1000, weight 1, touchpad:4000 | 60.0 | 0 (0.0) | 17.8 | 0 | 17.8 |

#### 60 Hz / 150 % · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 59.9 | 0 (0.0) | 17.9 | 14 | 33.6 |
| 1 | rows 1000, weight 1, fling:8000 | 60.1 | 0 (0.0) | 18.2 | 17 | 33.6 |
| 2 | rows 1000, weight 1, wheel:10 | 59.0 | 8 (59.6) | 37.1 | 31 | 33.8 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.6 | 3 (22.1) | 35.7 | 15 | 35.7 |

#### 60 Hz / 150 % · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 482/483 | 16.7 | 17.5 | 18.5 | 0.0 | 7.2/9.3 | 1.4/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.4 | 18.0 | 0.0 | 6.4/8.0 | 0.7/1.1 |
| 2 | rows 100000, weight 1, fling:8000 | 483/483 | 16.7 | 17.6 | 18.9 | 0.0 | 6.0/8.1 | 1.5/1.0 |
| 3 | rows 1000000, weight 1, fling:8000 | 483/483 | 16.7 | 17.6 | 18.4 | 0.0 | 6.7/8.9 | 0.8/1.1 |

#### 60 Hz / 150 % · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 483/483 | 16.7 | 17.4 | 17.8 | 0.0 | 6.2/8.4 | 1.1/1.0 |
| 1 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 18.7 | 0.0 | 6.1/8.1 | 1.3/0.9 |
| 2 | rows 10000, weight 3, fling:8000 | 483/483 | 16.7 | 17.9 | 18.4 | 0.0 | 6.2/9.3 | 0.6/1.6 |
| 3 | rows 10000, weight 2, fling:8000 | 483/483 | 16.7 | 18.2 | 18.7 | 0.0 | 5.7/9.2 | 1.1/3.5 |

#### 144 Hz / 100 % · D3D12 · artwork

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.0 | 0.0 | 2.1/5.7 | 1.1/1.1 |
| 1 | rows 10000, weight 1, uncached art ×1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.6 | 0.0 | 2.2/6.0 | 1.1/0.9 |
| 2 | rows 10000, weight 1, uncached art ×4, fling:8000 | 1159/1158 | 6.9 | 7.4 | 8.3 | 0.0 | 2.3/6.2 | 0.9/0.8 |
| 3 | rows 10000, weight 1, uncached art ×16, fling:8000 | 1158/1158 | 7.0 | 7.5 | 8.1 | 0.0 | 2.2/6.5 | 0.8/1.1 |

#### 144 Hz / 100 % · D3D12 · churn

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, churn/s 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.0 | 0.0 | 2.2/5.7 | 0.8/0.8 |
| 1 | rows 10000, weight 1, churn/s 10, fling:8000 | 1155/1158 | 6.9 | 8.0 | 29.7 | 22.36 | 2.1/5.8 | 22.7/0.7 |
| 2 | rows 10000, weight 1, churn/s 100 (ran 125), fling:8000 | 1158/1158 | 6.9 | 8.1 | 8.6 | 0.0 | 2.0/5.6 | 1.2/0.3 |
| 3 | rows 10000, weight 1, churn/s 1000, fling:8000 | 1158/1158 | 6.8 | 9.2 | 9.9 | 0.0 | 1.7/5.3 | 2.3/0.4 |

#### 144 Hz / 100 % · D3D12 · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1156/1156 | 6.9 | 8.2 | 15.0 | 7.47 | 1.9/5.6 | 1.8/8.8 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1156/1156 | 7.0 | 8.3 | 10.4 | 7.47 | 1.7/6.4 | 1.7/4.3 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1156/1156 | 6.6 | 21.1 | 21.7 | 2725.81 | 1.2/1.7 | 11.8/1.5 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 480/1156 | 14.7 | 48.5 | 70.6 | 5205.19 | 13.4/43.6 | 48.7/12.1 |

#### 144 Hz / 100 % · D3D12 · cpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.0 | 0.0 | 2.2/5.9 | 0.6/1.0 |
| 1 | rows 10000, weight 1, affinity 0x5, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.6 | 0.0 | 2.3/6.1 | 0.8/0.9 |
| 2 | rows 10000, weight 1, affinity 0x1, fling:8000 | 1159/1158 | 6.9 | 7.5 | 8.6 | 0.0 | 2.2/6.3 | 1.2/1.2 |
| 3 | rows 10000, weight 1, power saver, fling:8000 | 1158/1158 | 6.9 | 7.5 | 7.9 | 0.0 | 2.3/6.4 | 0.7/1.1 |
| 4 | rows 10000, weight 1, affinity 0x1, power saver, fling:8000 | 1158/1158 | 6.9 | 7.4 | 7.9 | 0.0 | 2.2/6.3 | 0.9/0.9 |

#### 144 Hz / 100 % · D3D12 · ext_churn

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, churn/s 3000, fling:8000 | 1157/1158 | 6.7 | 10.6 | 14.3 | 208.67 | 1.1/5.5 | 5.6/0.6 |
| 1 | rows 10000, weight 1, churn/s 10000, fling:8000 | 1156/1158 | 6.5 | 12.0 | 14.8 | 1691.71 | 0.8/5.3 | 6.5/0.4 |
| 2 | rows 10000, weight 1, churn/s 30000, fling:8000 | 1087/1158 | 6.6 | 16.2 | 19.9 | 3294.02 | 1.1/5.7 | 11.6/0.8 |

#### 144 Hz / 100 % · D3D12 · ext_combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000000, weight 2, churn/s 1000, uncached art ×4, affinity 0x1, power saver, fling:12000 | 322/1156 | 23.3 | 77.5 | 108.7 | 6459.86 | 1.2/8.4 | 42.9/30.6 |
| 1 | rows 10000000, weight 2, churn/s 3000, uncached art ×4, affinity 0x1, power saver, fling:12000 | 141/1156 | 22.7 | 141.8 | 141.9 | 7512.79 | 0.9/9.4 | 140.2/17.8 |
| 2 | rows 10000000, weight 2, churn/s 10000, uncached art ×4, affinity 0x1, power saver, fling:12000 | 42/1156 | 35.8 | 453.3 | 453.3 | 8125.43 | 1.9/8.5 | 467.6/31.1 |

#### 144 Hz / 100 % · D3D12 · ext_cpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 2, affinity 0x1, fling:12000 | 1154/1156 | 6.9 | 8.2 | 15.9 | 14.94 | 2.1/5.5 | 7.0/9.7 |
| 1 | rows 10000, weight 2, affinity 0x1, power saver, fling:12000 | 1156/1156 | 6.9 | 8.1 | 11.5 | 7.47 | 2.0/5.3 | 0.9/5.3 |
| 2 | rows 10000, weight 2, churn/s 100 (ran 125), affinity 0x1, power saver, fling:12000 | 1156/1156 | 7.0 | 8.4 | 11.1 | 7.47 | 1.6/5.8 | 1.4/4.0 |
| 3 | rows 10000, weight 2, churn/s 1000, affinity 0x1, power saver, fling:12000 | 1156/1156 | 7.1 | 9.3 | 10.7 | 7.47 | 0.9/5.1 | 2.6/2.4 |

#### 144 Hz / 100 % · D3D12 · ext_rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000000, weight 1, fling:12000 | 1155/1156 | 6.9 | 7.5 | 10.3 | 0.0 | 2.3/6.6 | 0.9/1.2 |
| 1 | rows 40000000, weight 1, fling:12000 | 1155/1156 | 7.0 | 7.5 | 8.0 | 0.0 | 2.3/5.8 | 0.8/0.8 |

#### 144 Hz / 100 % · D3D12 · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1209/1209 | 6.9 | 7.6 | 8.0 | 0.0 | 2.4/6.3 | 0.9/1.2 |
| 1 | rows 10000, weight 1, fling:2000 | 1180/1180 | 7.0 | 7.5 | 8.1 | 0.0 | 2.2/6.1 | 1.0/0.9 |
| 2 | rows 10000, weight 1, fling:4000 | 1165/1165 | 7.0 | 7.5 | 8.2 | 0.0 | 2.3/6.4 | 0.9/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.1 | 0.0 | 2.2/6.1 | 0.7/1.0 |
| 4 | rows 10000, weight 1, fling:12000 | 1156/1156 | 6.9 | 7.4 | 8.8 | 0.0 | 2.3/6.2 | 0.7/1.2 |

#### 144 Hz / 100 % · D3D12 · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.3 | 7.9 | 0.0 | 2.3/6.1 | 0.6/0.8 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1143/1158 | 7.0 | 13.9 | 14.1 | 111.79 | 2.3/8.0 | 8.0/1.9 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.4 | 21.6 | 983.73 | 0.8/1.2 | 10.2/1.0 |

#### 144 Hz / 100 % · D3D12 · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 142.7 | 2 (68.0) | 13.6 | 0 | 7.6 |
| 1 | rows 1000, weight 1, fling:8000 | 143.1 | 1 (51.1) | 14.1 | 1 | 14.1 |
| 2 | rows 1000, weight 1, wheel:10 | 143.9 | 0 (0.0) | 7.3 | 0 | 7.4 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.5 | 3 (22.0) | 14.2 | 1 | 14.2 |

#### 144 Hz / 100 % · D3D12 · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 122.5 | 40 (1256.8) | 22.4 | 667 | 23.6 |
| 1 | rows 1000, weight 1, fling:8000 | 78.3 | 88 (3899.9) | 21.7 | 654 | 28.6 |
| 2 | rows 1000, weight 1, wheel:10 | 62.5 | 652 (4852.5) | 25.6 | 725 | 25.6 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.5 | 665 (4888.5) | 25.9 | 736 | 25.9 |

#### 144 Hz / 100 % · D3D12 · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.0 | 0.0 | 2.3/6.3 | 0.8/0.9 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.8 | 0.0 | 2.3/6.5 | 1.0/0.9 |
| 2 | rows 100000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.2 | 0.0 | 2.1/6.2 | 0.7/1.0 |
| 3 | rows 1000000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.4 | 8.1 | 0.0 | 2.5/6.1 | 0.6/1.1 |

#### 144 Hz / 100 % · D3D12 · touchpad

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, touchpad:1000 | 1301/1301 | 6.9 | 7.3 | 8.8 | 0.0 | 2.3/6.3 | 0.5/1.0 |
| 1 | rows 10000, weight 1, touchpad:4000 | 1300/1301 | 6.9 | 7.4 | 7.9 | 0.0 | 2.4/6.2 | 0.6/1.0 |
| 2 | rows 10000, weight 1, touchpad:10000 | 1301/1301 | 6.9 | 7.5 | 8.8 | 0.0 | 2.4/6.6 | 0.5/1.0 |
| 3 | rows 10000, weight 1, touchpad:20000 | 1301/1301 | 6.9 | 7.5 | 7.8 | 0.0 | 2.2/5.5 | 0.5/0.9 |

#### 144 Hz / 100 % · D3D12 · warp_production

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, WARP, fling:2000 | 1180/1180 | 6.9 | 20.6 | 21.5 | 460.92 | 0.8/1.2 | 11.2/1.3 |
| 1 | rows 1000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.0 | 21.6 | 447.15 | 0.8/1.2 | 11.0/1.4 |
| 2 | rows 1000, weight 1, WARP, wheel:10 | 1287/1287 | 6.9 | 21.4 | 21.8 | 1696.26 | 0.8/1.1 | 10.0/1.1 |
| 3 | rows 1000, weight 1, WARP, touchpad:4000 | 1301/1301 | 6.9 | 21.4 | 21.7 | 955.67 | 0.8/1.2 | 10.1/1.2 |

#### 144 Hz / 100 % · D3D12 · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1158/1158 | 7.0 | 7.4 | 8.2 | 0.0 | 2.3/6.1 | 1.3/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 1156/1158 | 6.9 | 7.5 | 13.8 | 14.91 | 2.4/6.7 | 7.0/1.1 |
| 2 | rows 10000, weight 3, fling:8000 | 1158/1158 | 7.0 | 7.5 | 8.7 | 0.0 | 2.0/6.2 | 0.5/1.3 |
| 3 | rows 10000, weight 2, fling:8000 | 1159/1158 | 6.9 | 7.9 | 15.5 | 7.45 | 2.2/5.7 | 1.4/8.6 |

#### 144 Hz / 100 % · D3D12 · wheel

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, wheel:5 | 1273/1273 | 6.9 | 7.4 | 7.9 | 0.0 | 2.0/5.6 | 0.5/0.9 |
| 1 | rows 10000, weight 1, wheel:10 | 1288/1287 | 6.9 | 7.4 | 8.0 | 0.0 | 2.2/6.4 | 0.8/0.8 |
| 2 | rows 10000, weight 1, wheel:20 | 1295/1295 | 7.0 | 7.4 | 8.5 | 0.0 | 2.1/6.1 | 0.5/0.8 |
| 3 | rows 10000, weight 1, wheel:40 | 1298/1298 | 7.0 | 7.5 | 8.7 | 0.0 | 2.2/6.5 | 0.9/0.9 |

#### 60 Hz / 100 % · D3D12 · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 482/482 | 16.7 | 18.3 | 23.2 | 0.0 | 5.4/7.6 | 1.6/6.9 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 482/482 | 16.6 | 18.5 | 21.3 | 0.0 | 4.7/6.8 | 1.4/4.9 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 484/482 | 16.5 | 49.9 | 50.0 | 1142.61 | 1.3/1.8 | 24.3/1.1 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 447/482 | 16.5 | 56.8 | 72.5 | 634.78 | 13.3/29.7 | 39.1/7.2 |

#### 60 Hz / 100 % · D3D12 · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 504/504 | 16.7 | 17.3 | 17.9 | 0.0 | 6.4/7.6 | 0.7/1.0 |
| 1 | rows 10000, weight 1, fling:2000 | 492/492 | 16.7 | 17.5 | 17.7 | 0.0 | 6.4/7.0 | 0.9/0.9 |
| 2 | rows 10000, weight 1, fling:4000 | 485/486 | 16.7 | 17.2 | 17.8 | 0.0 | 6.3/8.7 | 0.7/0.8 |
| 3 | rows 10000, weight 1, fling:8000 | 482/483 | 16.7 | 17.4 | 17.8 | 0.0 | 6.5/9.0 | 0.7/1.0 |
| 4 | rows 10000, weight 1, fling:12000 | 482/482 | 16.7 | 17.2 | 17.4 | 0.0 | 6.2/7.2 | 0.9/0.6 |

#### 60 Hz / 100 % · D3D12 · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.3 | 17.8 | 0.0 | 6.5/7.3 | 0.7/0.8 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 483/483 | 16.7 | 17.1 | 18.5 | 0.0 | 7.2/9.1 | 4.8/1.7 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 483/483 | 16.7 | 50.4 | 50.8 | 394.98 | 0.9/1.2 | 22.1/1.1 |

#### 60 Hz / 100 % · D3D12 · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 60.0 | 0 (0.0) | 17.1 | 0 | 17.1 |
| 1 | rows 1000, weight 1, fling:8000 | 58.8 | 2 (73.5) | 33.1 | 0 | 17.3 |
| 2 | rows 1000, weight 1, wheel:10 | 59.9 | 0 (0.0) | 17.2 | 0 | 17.2 |
| 3 | rows 1000, weight 1, touchpad:4000 | 60.0 | 0 (0.0) | 17.0 | 0 | 17.3 |

#### 60 Hz / 100 % · D3D12 · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 60.0 | 0 (0.0) | 17.3 | 6 | 33.7 |
| 1 | rows 1000, weight 1, fling:8000 | 57.7 | 2 (135.8) | 36.4 | 6 | 36.4 |
| 2 | rows 1000, weight 1, wheel:10 | 59.3 | 5 (37.2) | 38.1 | 8 | 38.1 |
| 3 | rows 1000, weight 1, touchpad:4000 | 59.3 | 5 (36.8) | 36.5 | 4 | 33.4 |

#### 60 Hz / 100 % · D3D12 · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 483/483 | 16.7 | 17.5 | 18.4 | 0.0 | 6.4/9.2 | 1.6/1.1 |
| 1 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.5 | 18.0 | 0.0 | 6.4/8.6 | 0.8/1.0 |
| 2 | rows 100000, weight 1, fling:8000 | 482/483 | 16.7 | 17.5 | 18.4 | 0.0 | 6.5/8.0 | 1.1/1.1 |
| 3 | rows 1000000, weight 1, fling:8000 | 482/483 | 16.7 | 17.5 | 18.0 | 0.0 | 6.3/7.3 | 1.0/0.8 |

#### 60 Hz / 100 % · D3D12 · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 483/483 | 16.7 | 17.2 | 18.3 | 0.0 | 6.3/7.0 | 1.1/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 483/483 | 16.7 | 17.4 | 17.8 | 0.0 | 6.3/7.8 | 1.1/0.9 |
| 2 | rows 10000, weight 3, fling:8000 | 482/483 | 16.7 | 17.7 | 18.0 | 0.0 | 6.5/8.3 | 0.8/1.1 |
| 3 | rows 10000, weight 2, fling:8000 | 482/483 | 16.7 | 17.7 | 26.3 | 7.45 | 6.1/7.2 | 0.9/9.1 |

#### 144 Hz / 125 % · D3D12 · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1152/1156 | 6.9 | 8.5 | 37.1 | 37.34 | 1.9/5.8 | 24.1/30.3 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1156/1156 | 6.9 | 8.3 | 13.8 | 7.47 | 1.2/5.5 | 1.9/7.5 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1158/1156 | 6.7 | 21.2 | 21.8 | 2546.37 | 1.6/2.0 | 12.2/1.5 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 379/1156 | 20.8 | 64.5 | 65.1 | 5839.99 | 17.3/51.3 | 36.8/6.5 |

#### 144 Hz / 125 % · D3D12 · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1209/1209 | 6.9 | 7.7 | 8.1 | 0.0 | 2.5/6.2 | 1.1/0.8 |
| 1 | rows 10000, weight 1, fling:2000 | 1179/1180 | 7.0 | 7.6 | 8.2 | 0.0 | 2.2/6.0 | 1.0/1.0 |
| 2 | rows 10000, weight 1, fling:4000 | 1165/1165 | 7.0 | 7.4 | 7.9 | 0.0 | 2.3/5.8 | 0.9/0.8 |
| 3 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.5 | 8.5 | 0.0 | 2.1/5.7 | 0.9/1.1 |
| 4 | rows 10000, weight 1, fling:12000 | 1156/1156 | 6.9 | 7.4 | 8.2 | 0.0 | 2.0/5.6 | 1.1/0.9 |

#### 144 Hz / 125 % · D3D12 · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 7.9 | 0.0 | 2.5/6.3 | 0.6/0.8 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1155/1158 | 7.0 | 7.5 | 14.2 | 22.36 | 2.5/7.2 | 7.8/1.9 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.4 | 21.7 | 685.63 | 1.1/1.5 | 10.2/1.3 |

#### 144 Hz / 125 % · D3D12 · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 143.4 | 1 (30.4) | 13.6 | 0 | 7.4 |
| 1 | rows 1000, weight 1, fling:8000 | 143.8 | 0 (0.0) | 7.4 | 0 | 7.4 |
| 2 | rows 1000, weight 1, wheel:10 | 143.7 | 1 (7.4) | 13.9 | 0 | 7.3 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.4 | 4 (29.4) | 14.0 | 0 | 7.6 |

#### 144 Hz / 125 % · D3D12 · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 111.3 | 71 (1943.5) | 21.8 | 669 | 24.4 |
| 1 | rows 1000, weight 1, fling:8000 | 82.9 | 50 (3552.1) | 23.9 | 652 | 27.1 |
| 2 | rows 1000, weight 1, wheel:10 | 62.3 | 655 (4865.5) | 32.0 | 727 | 32.0 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.3 | 666 (4892.9) | 34.7 | 737 | 31.5 |

#### 144 Hz / 125 % · D3D12 · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.2 | 0.0 | 1.6/5.5 | 0.8/1.0 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.3 | 0.0 | 2.1/6.3 | 1.0/1.0 |
| 2 | rows 100000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.4 | 7.9 | 0.0 | 2.4/5.9 | 0.6/0.9 |
| 3 | rows 1000000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.4 | 8.0 | 0.0 | 2.5/6.2 | 0.8/0.9 |

#### 144 Hz / 125 % · D3D12 · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1158/1158 | 7.0 | 7.4 | 7.9 | 0.0 | 2.2/6.0 | 0.9/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.3 | 8.0 | 0.0 | 2.1/6.0 | 0.6/0.9 |
| 2 | rows 10000, weight 3, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.9 | 0.0 | 2.1/5.5 | 1.1/1.4 |
| 3 | rows 10000, weight 2, fling:8000 | 1155/1158 | 6.9 | 7.7 | 33.5 | 29.81 | 2.4/5.6 | 20.7/27.0 |

#### 144 Hz / 150 % · D3D12 · combined

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000000, weight 2, churn/s 100 (ran 125), fling:12000 | 1155/1156 | 6.9 | 8.3 | 20.5 | 14.94 | 1.3/4.6 | 6.9/13.0 |
| 1 | rows 1000000, weight 2, churn/s 100 (ran 125), uncached art ×4, affinity 0x1, fling:12000 | 1156/1156 | 6.9 | 8.3 | 13.7 | 7.47 | 1.0/4.6 | 1.9/6.8 |
| 2 | rows 1000000, weight 2, churn/s 100 (ran 125), WARP, fling:12000 | 1157/1156 | 6.5 | 21.3 | 21.6 | 2606.35 | 2.2/2.9 | 12.7/2.0 |
| 3 | rows 1000000, weight 2, churn/s 1000, uncached art ×4, WARP, affinity 0x1, fling:12000 | 334/1156 | 20.9 | 98.8 | 139.4 | 6355.25 | 19.3/60.2 | 70.9/6.4 |

#### 144 Hz / 150 % · D3D12 · fling

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:1000 | 1209/1209 | 6.9 | 7.7 | 8.4 | 0.0 | 1.6/5.4 | 1.3/0.9 |
| 1 | rows 10000, weight 1, fling:2000 | 1179/1180 | 7.0 | 7.5 | 7.9 | 0.0 | 1.5/5.3 | 1.0/0.9 |
| 2 | rows 10000, weight 1, fling:4000 | 1166/1165 | 6.9 | 7.4 | 8.1 | 0.0 | 1.6/5.2 | 1.0/0.9 |
| 3 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.5 | 8.2 | 0.0 | 1.5/5.3 | 1.0/1.0 |
| 4 | rows 10000, weight 1, fling:12000 | 1156/1156 | 6.9 | 7.5 | 8.1 | 0.0 | 2.0/6.1 | 0.8/0.9 |

#### 144 Hz / 150 % · D3D12 · gpu

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.5 | 0.0 | 1.5/5.8 | 1.6/1.0 |
| 1 | rows 10000, weight 1, iGPU, fling:8000 | 1156/1158 | 6.9 | 7.4 | 14.2 | 14.91 | 3.2/6.1 | 8.0/2.0 |
| 2 | rows 10000, weight 1, WARP, fling:8000 | 1158/1158 | 6.9 | 21.7 | 22.0 | 879.4 | 1.5/2.0 | 11.6/2.0 |

#### 144 Hz / 150 % · D3D12 · llvmpipe_hw_control

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 143.9 | 0 (0.0) | 7.4 | 0 | 7.4 |
| 1 | rows 1000, weight 1, fling:8000 | 143.8 | 0 (0.0) | 7.3 | 0 | 7.3 |
| 2 | rows 1000, weight 1, wheel:10 | 143.7 | 1 (7.4) | 13.9 | 0 | 8.6 |
| 3 | rows 1000, weight 1, touchpad:4000 | 143.6 | 2 (14.7) | 13.9 | 0 | 7.4 |

#### 144 Hz / 150 % · D3D12 · llvmpipe_production

| step | load | list updates/s | list skipped (/min moving) | longest freeze ms | GUI clock skipped | GUI clock max ms |
|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:2000 | 106.6 | 69 (2218.5) | 28.7 | 666 | 35.0 |
| 1 | rows 1000, weight 1, fling:8000 | 84.4 | 68 (3479.6) | 22.7 | 651 | 27.6 |
| 2 | rows 1000, weight 1, wheel:10 | 62.6 | 655 (4860.1) | 24.4 | 725 | 24.4 |
| 3 | rows 1000, weight 1, touchpad:4000 | 62.2 | 665 (4886.6) | 46.1 | 735 | 27.7 |

#### 144 Hz / 150 % · D3D12 · rows

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 1000, weight 1, fling:8000 | 1158/1158 | 7.0 | 7.3 | 7.9 | 0.0 | 1.5/5.5 | 0.8/1.0 |
| 1 | rows 10000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 7.9 | 0.0 | 1.4/5.4 | 1.0/0.9 |
| 2 | rows 100000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.0 | 0.0 | 1.6/4.9 | 0.5/1.0 |
| 3 | rows 1000000, weight 1, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.2 | 0.0 | 1.5/5.2 | 1.1/0.9 |

#### 144 Hz / 150 % · D3D12 · weight

| step | load | frames/expected | p50 | p99 | max | dropped/min | GPU busy p50/p99 | ring GUI/render excess |
|---|---|---|---|---|---|---|---|---|
| 0 | rows 10000, weight 0, fling:8000 | 1158/1158 | 6.9 | 7.4 | 8.3 | 0.0 | 1.5/4.9 | 1.4/0.8 |
| 1 | rows 10000, weight 1, fling:8000 | 1159/1158 | 6.9 | 7.5 | 8.4 | 0.0 | 1.6/5.3 | 1.4/1.0 |
| 2 | rows 10000, weight 3, fling:8000 | 1158/1158 | 7.0 | 7.5 | 8.6 | 0.0 | 2.3/5.6 | 0.6/1.0 |
| 3 | rows 10000, weight 2, fling:8000 | 1157/1158 | 6.9 | 7.8 | 28.6 | 22.36 | 1.6/5.1 | 14.7/22.0 |
