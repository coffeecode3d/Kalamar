# Controls

> **Shared with the upstream project.** The engine is a fork of totk-pc-recomp, and parts of this page describe its
> *Tears of the Kingdom* targets only: the table of actions below, "Xbox button prompts", "PC button prompts", "Mouse
> camera" and "Mouse in the game's menus" (each says so). For Kalamar's target, *Splatoon 3*: "Splatoon 3: keyboard
> and mouse" and "Mouse as the gyro"; the rest (layouts, window keys, remapping, scripted input, RenderDoc capture)
> applies to every target. The settings file is `kalamar.ini` (`totk.ini` for the upstream's targets).

Input comes from the first SDL3 gamepad, the keyboard and the mouse, merged into one Switch controller (`runtime/input_win.c`). Buttons are ORed; each stick takes the source with the larger deflection. Keys and mouse buttons count only while the game window is in the foreground. `KLM_INPUT=none` disables both (automated runs). `KLM_*` settings can also be set in `totk.ini` next to `totk.exe` (`input = none`, `motion = 0`, `rumble = 0`); the environment overrides the file. `KLM_RUMBLE_STRENGTH` is the rumble strength in percent (0 to 300, default 130; 0 is the same as `rumble = 0`). `KLM_RUMBLE_CURVE` is the exponent applied to the amplitude before the strength (0.2 to 1.5, default 0.6; lower makes weak pulses stronger, 1 is linear). `KLM_RUMBLE_TRIGGERS=1` also drives the impulse triggers of an Xbox pad.

`KLM_PAD_LAYOUT` (`pad_layout`, the *Button layout* row on the Controls tab, live) picks which face button is Switch A:

- `nintendo`: by position, as on a Switch (the table below).
- `xbox`: south A, east B, west X, north Y, as an Xbox pad labels them; on a PlayStation pad cross is A, circle B, square X, triangle Y.
- `auto` (default): `nintendo` for a pad whose east button SDL reports labeled A (Switch Pro Controller, Nintendo-layout pads), `xbox` for the others.

The log names the layout in effect (`input: gamepad layout ...`); the menu shows it under *Automatic*. Which controller's buttons the game's prompts show is `KLM_PROMPTS` (see Xbox button prompts below). 0.1.9's `pad_layout = xbox_prompts` is read as `pad_layout = nintendo` with `prompts = xbox`, and the menu writes it that way.

The upstream target's default keys and actions (*Tears of the Kingdom*):

| Switch | Gamepad (by position, Nintendo layout) | Keyboard and mouse | Action in the world |
|---|---|---|---|
| Left stick | left stick | W A S D | move |
| Right stick | right stick | mouse; arrow keys as a second source | camera |
| Y | west | left mouse button, T | attack |
| ZL | left trigger (over 50%) | right mouse button, Z | shield, lock on |
| ZR | right trigger (over 50%) | F, X | bow |
| X | north | Space | jump |
| B | south | Left Shift | run, cancel |
| A | east | E | interact, pick up |
| L | left shoulder | Q | abilities |
| R | right shoulder | R | throw weapon |
| Left stick click | left stick click | Left Ctrl | crouch |
| Right stick click | right stick click | C | first-person view |
| D-pad left, up, right, down | D-pad | 1 2 3 4 | quick-select |
| Plus | Start | Tab | menu, inventory |
| Minus | Back | M | map |
| Right stick shaken left and right | shake the right stick | G | detach a fused part from what Ultrahand holds |

Esc is unbound in the game; it opens the settings menu and releases the mouse cursor (see Mouse camera). Z and X duplicate ZL and ZR so menu pages turn with two neighboring keys; the menu tabs (L, R) are Q and R. T duplicates Y for the game's menus, where the left mouse button is A while the pointer is shown (sort the inventory with T). The mouse wheel zooms the Purah Pad's map and is unbound elsewhere. The bindings are the tables of `input_map` in `runtime/input_map.c`: the keys (platform-neutral key ids) and one gamepad table per layout (gamepad inputs by position); `runtime/input_win.c` translates Win32 virtual keys and SDL buttons.

## Splatoon 3: keyboard and mouse

A Splatoon 3 build (`rt_target.family`) starts from a PC shooter's layout instead of the table above (`SPLAT3_KEYS` in `runtime/input_map.c`; the remapping rows and `bind_` lines work on top as before). The mouse aims through the game's motion controls (next section).

| Switch | Keyboard and mouse | In the game |
|---|---|---|
| Left stick | W A S D | move |
| Motion (gyro) | mouse | aim, camera |
| ZR | left click | shoot |
| R | right click, R | sub weapon |
| ZL | Left Shift | squid / swim |
| B | Space | jump (back in menus) |
| Right stick click | E, middle click | special weapon |
| Y | Q | recenter the camera |
| A | F | confirm, interact |
| X | M, Tab | map |
| Plus | Enter | menu (Esc opens Kalamar's settings menu and cannot be bound) |
| Minus | Backspace | |
| L | Z | tabs in menus |
| Left stick click | Left Ctrl | |
| D-pad left, up, right, down | arrow keys | signals: up "This way!", down "Booyah!" |
| Right stick | I J K L | the camera while the game's motion controls are off |

The mouse wheel is unbound by default; `wheel_up` and `wheel_down` bind it like keys ("Remapping"). Menus work with W A S D or the arrows, F and Space; the mouse does not point at menu items (the game's touch screen is not mapped, and the TotK menu cursor does not apply).

Spectating a private battle (a spectator's seat, [auto-match.md](auto-match.md), "Spectators"): the game's camera directs itself until a player is picked. Measured with the gamepad buttons: hold Left Shift (ZL) to see which button follows which player (the arrow keys for Alpha's players, F for Bravo's first), hold that key about half a second to follow the player, hold Z (L) for the top-down view, Tab or M (X) for the map. Short taps did nothing, on the pad as on the keyboard; with the keyboard only the map (Tab, M) was seen working, the held keys were not tried.

## Mouse as the gyro

`mouse_gyro = auto` (the default) turns the mouse into the six-axis sensor on a Splatoon 3 build and leaves it the right stick on Tears of the Kingdom; `1` and `0` force either. The game polls `nn::hid::GetSixAxisSensorStates(state *, 16, handle)` once per frame from its main thread (about 60 a second) for the full-key controller's sensor (handle `0x20003`) and reads the angular velocity, the direction matrix and the acceleration of each 0x60-byte state. The motion thread (`runtime/input.c`, `mouse_gyro_tick`) takes the raw mouse counts every 4 ms and turns them, on a 5 ms grid by elapsed time (200 Hz, `delta_ns` 5000000, consecutive sampling numbers, as the Switch's sensor), into samples of a virtual controller (`runtime/mouse_gyro.c`): each sample carries an equal share of the counts since the previous one, so none is lost between frames. Mouse right turns right (angular velocity about the world's vertical axis), mouse up looks up (about the controller's x axis). The direction matrix and the gravity vector come from the same yaw and pitch, so they agree with the integrated angular velocity (`--probe-mgyro` checks it); pitch stops at `mouse_gyro_pitch` degrees. The conversion adds no acceleration or smoothing; the game's own smoothing of the sensor is switched off for the mouse by `mouse_native` (below).

- `mouse_gyro_sens`: degrees of controller turn per count (default 0.04, 0.002 to 1). The game's own camera sensitivity in its Options multiplies it.
- `mouse_gyro_x`, `mouse_gyro_y`: a multiplier per axis; `mouse_gyro_invert_y = 1` inverts the vertical axis.
- `mouse_gyro_recenter_key`: a key that levels the virtual controller. The Y button (Q) does it too, since it recenters the game's camera.
- `motion = 0` turns every motion source off, the mouse's included. A real gamepad's gyro (when SDL exposes it) is read instead while the gamepad is the device used last, and the mouse takes over again when a key or the mouse moves.
- The game's Options must have motion controls on (a new profile has them on: the tutorial teaches "tilt the controller to move the reticle" and calibrates by asking to look up and down); with them off the game ignores the sensor and the mouse does nothing, so set `mouse_gyro = 0` (the mouse is then a right stick) or turn them on. `mouse_sens = 0` still turns the mouse off entirely.
- `mouse_native` (default `1`, live; Splatoon 3 11.3.0): native mouse aiming, below.

### Native mouse aiming

The game smooths a real controller's motion in three places, which made the mouse feel like "aiming through jello": its gyro processor hands the cameras a filtered attitude (a state about two samples behind the newest, blended into a prediction of the previous output with a weight that drops to 0.2 when the attitude jumps and recovers by 0.2 a frame), the player camera eases its pitch toward the pitch it computes by about 0.3 of the distance a frame, and it damps a yaw turn that follows motion the other way (the gain drops as low as 0.1 and recovers over about ten frames, the cut part given back later). Yaw comes from the newest state's angular velocity, so it was already whole in one frame; pitch comes from the attitude and lagged by about eight frames. While the mouse's virtual controller is the sensor the game reads and `mouse_native = 1`, `runtime/mouse_native.c` turns the three off: a switch of the game's own gyro tuning data makes the processor hand out the newest attitude, and two hooks (`recomp/patches-splatoon3-v1130.txt`, set `mouse_native`) make the camera take its pitch target whole and the yaw turn undamped; the pitch is also made linear (below). The game keeps everything else: the motion sensitivity of its Options, its pitch limits, the Y recentre, collisions, the camera modes and what it sends to the other players. A gamepad's gyro gets the game's smoothing and pitch curve back as soon as it is the source again. `mouse_native = 0` gives the mouse the game's smoothing and curve too. 11.2.0 has neither the switch's address nor the hooks: there the setting does nothing.

Measured with the bench (`KLM_MGYRO_BENCH=native`, test range, 0.04 degrees per count, the game's default motion sensitivity; both settings in one run):

| | `mouse_native = 0` | `mouse_native = 1` |
| --- | --- | --- |
| Pitch step (8 and 2 degrees of controller pitch) | 2 to 7% of the turn in each of the first three frames, 90% only 10 to 11 frames after the first move | whole in the first frame that moves, the same frame as a yaw step |
| Yaw step | whole in one frame | whole in one frame |
| Yaw back and forth (240 counts at 1200 counts/s, five cycles) | best fit 3 frames late with 0.86 of the gain, 19% of the swing left unexplained | best fit as late as a yaw step, full gain, 0.0% unexplained |
| Circles of 250 counts in 1 s and 0.5 s: pitch delay | 8 frames | as late as the yaw (2 frames on that aim vector) |

#### Linear pitch

With the smoothing off the pitch still followed the game's pitch curve, which maps the controller's tilt to the camera's pitch: 25-count steps over the whole range turned the view by 0.030 to 0.112 degrees per count between 55 degrees up and 55 down, depending on where the pitch was (0.098 at level just after a Y), and less and less toward the top limit, while yaw turned 0.0957 everywhere (0.04 degrees per count and the bench profile's motion sensitivity, at which the game turns its yaw 2.393 degrees per degree of controller turn). On circles that left about 4.5% of the pitch's swing that no straight fit explains, at every speed.

With `mouse_native = 1` the mouse's pitch turns by the yaw's own degrees per count over the whole range, in degrees of the view drawn. Three more sites of the same set do it: one reads the yaw's gain where the camera applies it, and two take the place of the camera's calls of its pitch curve, where the host calls the curve and the camera's pose function itself and hands out the pitch whose pose puts the view where the counts say; each frame it reads the view the camera drew and takes back in the next frame what the camera's later placement of its target added, including the slow settling of that placement after the mouse stops (`runtime/mouse_native.c`, part 4). The game keeps its limits (on the bench 60.4 degrees up and 72 to 73 down, where the view goes on settling by itself for a few seconds), the Y recentre, collisions and the camera modes; motion past a limit is dropped, so a turn back moves the view at once. The right stick's vertical aim, which the camera adds to the tilt before its curve, goes the same way while the mouse is the source (not measured).

When the game moves the camera's pitch itself, the linear pitch starts again from the view drawn. After the Y recentre it takes the pitch the game's curve gives (level); after anything else (a write of the pitch by the game, another camera, the setting or the mouse coming back as the source, a jump of the curve's input that the virtual controller's own turn does not explain) it keeps the pitch as it is. The curve's own result is no guide there: the virtual controller follows the view (below), not the curve, and once a Y has moved the curve's base the curve puts the view far from the one drawn (a median of 15 and 27 degrees over two runs of the bench's tests of play situations, up to 42). Each such frame, but one that only starts the linear pitch again, logs `mouse native: pitch kept through ...` with that distance (the first 16, then one in 256), so a player's log shows where the game moves the pitch itself. In three scripted local 1v1 matches (two with the mouse moving throughout, `KLM_MGYRO_BENCH=match` and then `matchold` with 0.3.4's behavior, one with the mouse still), that happened when the battle began, with a new camera whose pitch the game set level (the curve's result 24 and 15 degrees above it in two of them, level in the third, where the game also moved the base), and at the result screens (22 degrees above); during the 180 s of each battle, never. In the first one's battle, with today's behavior, the view ended each window of the bench's motion within 0.4 degree of where its counts put it.

Releases 0.3.3 and 0.3.4 took the curve's result in all those frames, and counted any change of the curve's input over 30 degrees as a jump: a vertical flick of 30 degrees of controller pitch in one frame (750 counts at 0.04 degrees per count, 72 degrees of view), or fewer counts per frame gathered across a frame hitch, moved the view by that distance instead of by its counts. Measured with the bench (`KLM_MGYRO_BENCH=flick`, test range, 0.0318 degrees per count; 0.3.4's behavior from the bench's switch `mgb_native_hold`):

| | 0.3.4 | Now |
| --- | --- | --- |
| A flick of 1000 counts (76 degrees of view) in one frame and back in one frame, from 30 degrees down, 30 up and 15 or 20 up: where the view ends against where it started, in two runs | 32.8 down, 19.2 up, 25.2 down; 16.1 down, 37.2 up, 25.7 down | where it started (within 0.01 degree); 3.7, 2.6 and 5.7 up |
| 1050 counts made while the game is held in a 200 ms hitch (80 degrees of view in the frame after it), and back over 6 frames | 22.2, 10.5 and 23.4 down | 2.9, 0.4 and 4.1 up |
| The flick after a Y with the view there (both legs reach a limit, and the part past it is dropped): where the view ends against where the counts and the limit put it (first run) | 29.6 up, 4.0 down, 4.1 down | there (within 0.01 degree) |

A turn of 76 degrees in a single frame still ends a few degrees off in some places (the second run): the linear pitch scales a frame's turn by how the view drawn followed the pose in the frame before, and near the bottom limit, where the camera met the floor and the view drawn stopped 10 degrees short of the pose's, that ratio was 13% off for the frame that came back. Steps of 200 counts in a frame (15 to 19 degrees of view) are whole and exact (the tables below).

When the player respawns, the game sets the new camera's pitch level and then holds the camera's turns for 59 frames, about a second, while the spawn point's view opens: it scales each frame's turn by 0, so the yaw does not turn. The linear pitch keeps the game's pitch through those frames and drops what the mouse turns in them, as the yaw does, then starts again from there; each hold logs `mouse native: pitch kept while the game holds the camera's turns`. Release 0.3.7 took that yaw gain of 0 for one not measured yet and handed out the curve's result in those frames, and the next frame kept it, so every respawn turned the view to wherever the curve put the virtual controller, which follows the view and not the curve. Measured with the bench in two scripted local 1v1 matches on a stage whose middle is water for the first minute (`KLM_MGYRO_BENCH=respawnrun`, and `respawnrunold` for 0.3.7's handling; the player walks from its spawn point into the water and respawns six times, the mouse still, turning up 24 degrees while the player is splatted, going up and down then, or turning up 22 degrees during the hold; ZR launches the player from the spawn point):

| | 0.3.7 | Now |
| --- | --- | --- |
| The view the spawn point's camera drew in the frame after the hold (the game's level pitch there: 14.5 degrees down) | 30.7 to 53.2 degrees down | 14.50 down every time |
| The human form's view 2 s after the landing, against the level view the game set (7.5 down) plus the counts made after the hold | 20.4, 24.5, 33.5, 40.5, 50.3 and 51.2 degrees lower | level five times, 0.2 degree higher once |

Whether 0.3.7 turned the view up or down depends on where the virtual controller was. Falling into the water turns the view down to 41 degrees below level and the controller follows it, so on these respawns the curve's result was below level; with a level view and no Y pressed in the match it is 15 degrees above the view, and after a Y the two can be up to 42 degrees apart. A respawn after being splatted by another player was not measured. A super jump back to the spawn point (X, then the spawn point) holds no turns: nine jumps in a match never resynced the linear pitch, but each landing left the view 0.9 to 3.7 degrees lower than before the jump (the mouse still, or going round closed motions), the aim starting again from the view the landing camera draws while it still settles.

Measured with the bench (`KLM_MGYRO_BENCH=sweep,step,vsine,circle,reverse,recenter,limits`, test range, 0.04 degrees per count, the view's pitch read from the camera's own view direction; each circle and up-and-down test starts from a level camera):

| | Game's pitch curve (smoothing off) | Linear pitch |
| --- | --- | --- |
| Pitch degrees per count, 25-count steps over the range both ways | 0.030 to 0.112 between 55 up and 55 down, 0 at the top limit | 0.0951 to 0.0966 from the top limit to 71 down (yaw 0.0957, at most 0.94% off), 0.0951 to 0.0964 between 55 up and 55 down |
| Pitch steps of 50 and 200 counts | as the steps above | 0.0957 degrees per count, 99.3 to 100.3% of the turn in the first frame that moves |
| Circles of 250 counts, 2 s to 0.33 s a turn: pitch left after a straight fit at the yaw's gain | 4.4 to 4.5% | 0.1 to 0.6% |
| Circles of 60 counts in 0.5 s | 2.8% | 0.1% |
| Up and down only, 60 to 500 counts, 2 s to 0.5 s | not measured | 0.04 to 0.2% |
| Lag behind the counts, pitch and yaw | 1 read | 1 read |

#### Squid form and the virtual controller

Squid form has a camera of its own: it sits lower behind its target (about 1 unit above it instead of 2), maps the game's pitch below level about as the human camera does and above level to a view about 3.4 times narrower, its top near 16 degrees up. When the form changes the game keeps its pitch, and the view goes over to the other camera's in 22 to 40 frames. The linear pitch keeps the game's pitch through such a change too (`runtime/mouse_native.c`, part 4). In squid form, and while the camera changes fast as in a change of form, each turn of the mouse moves the pitch as the human form's camera would (a table of that camera's pose, taken while it held still): the human form's view ends where the counts put it, whatever form they were made in, and a turn and its way back leave the pitch where it was, as a real controller's tilt does with the game's own aiming. The squid camera's view therefore turns about 0.85 times the counts below level and about 3.4 times less above it, as the game's own aiming turns it; the human form's view turns by the counts, vertically as horizontally. While the camera changes slowly, as when climbing a wall, each turn moves the view drawn by its counts while the climbing camera is as wide as the human one, and as the human camera would while it is as narrow as the squid one. During the change itself the view drawn goes over from one camera to the other as the game moves it: leaving squid form near level it rises up to 4.5 degrees above where it ends and comes back, with the game's own aiming too.

Releases 0.3.3 and 0.3.4 turned the squid camera's view by the counts instead, and during a change of form moved the pitch as the camera before the change did. A turn up made in squid form above level, or made in the half second after ZL was let go, then moved the human form's view about 3.4 times as far once the form came back, and the camera looked up by itself when the player left squid form. Measured with the bench (`KLM_MGYRO_BENCH=surface,swim`, test range, 0.0318 degrees per count; 0.3.4's behavior from the bench's switches `mgb_native_ref` and `mgb_native_hold`, the game's own aiming from `mouse_native = 0`): from a level camera or one aimed 20 or 40 degrees up, ZL held 100 frames, a vertical turn in squid form once its camera settled, the release, and from it the mouse still, a turn up or down over half a second, or a small closed circle (60 counts in 0.5 s); the human form's view 60 frames after the release against the view before ZL plus the counts:

| | 0.3.4 | Now | Game's own aiming |
| --- | --- | --- | --- |
| 3, 6, 12 and 25 degrees of counts up in squid form, from level | 8.3, 16.6, 30.9 and 42.2 degrees higher (the last at the top limit) | within 0.14 | within 1.1 |
| The same from 20 degrees up | 7.6, 15.0, 29.0 and 22.4 higher | 0.08 to 0.66 lower | 0.27 and 0.64 lower (3 and 6 degrees) |
| 6 degrees of counts down in squid form, from level and from 20 up | 1.0 and 14.7 lower | within 0.04 | 0.3 lower, 0.5 higher |
| 60 counts up (4.6 degrees) over the half second from the release, from level, 20 and 40 up | 7.3, 10.7 and 10.6 higher | within 0.18 | |
| 150 counts up (11.4 degrees) over that half second, from level and 20 up | 26.8 and 26.6 higher | within 0.25 | |
| 60 counts down over that half second, from level and 20 up | 0.6 and 10.8 lower | within 0.02 | |
| The small closed circle from the release, from level, 20 up and 20 down | 6.5 and 0.5 higher, 0.0 | within 0.05 | |
| A swim in ink with the stick forward, let go of while moving (from level and 20 up, the mouse still, 60 counts up from the release, 6 degrees in squid form) | up to 1.3 higher | within 0.42 | |
| The mouse still from start to end, from level, 20 and 40 up | within 0.09 | within 0.04 | within 0.05 (level and 20 up) |

The virtual controller's tilt follows the view: each frame it turns toward the view's pitch over the yaw's gain, by 1 degree at most and toward a pitch at least 10 degrees inside `mouse_gyro_pitch`, and the linear pitch takes those turns out of what it reads.

Release 0.3.2 had two faults here. Every turn dropped at a pitch limit or by a change of the camera left the virtual controller further from the view, and over a game it reached `mouse_gyro_pitch`: the pitch then no longer answered the mouse that way, past 84 degrees the yaw no longer turned the camera (the controller's yaw goes on its own vertical axis divided by the cosine of its pitch, which stops at 0.1), and Y no longer levelled the view; changing `mouse_gyro_pitch` restarted the controller and freed it for a while. And through a change of form the linear pitch held the view rather than the game's pitch: it fought the change and drove the pitch to its limits, which dropped more turns, and the view jumped by up to 26 degrees when the form came back. A first fix kept the game's pitch but moved it during the change along the pose of the moment, which moves the pitch further for the same counts near the squid camera's top than near the human one's: a small closed circle of the mouse across each change moved the pitch by up to 0.14 and 17 changes each way took a level view to 55 degrees up.

Measured with the bench (`KLM_MGYRO_BENCH=squid,drift,ink,wall,sweep,step,vsine,circle,reverse,recenter,limits`, test range) at the settings of the player who reported the faults: 15 cm per turn at 800 DPI (0.0318 degrees per count, so 0.0762 degrees of view per count at the game's default motion sensitivity) and `mouse_gyro_pitch = 89`. 0.3.2's behavior comes from the bench's own switch, `mgb_native_keep`, with today's other code; the game's own aiming from a run with `mouse_native = 0`. Steps are read as the median of three reads before and three reads 11 to 13 after them, so a one-frame jump of the game's own camera is not counted. The squid test changes form 51 times each way, 17 at level, 17 near the top (about 40 degrees up in human form) and 17 near the bottom (about 55 down), with a small closed circle of the mouse (60 counts in 0.5 s) across each change and steps of 100 counts (yaw) and 50 (pitch) in each form; 0.3.2 gets 6 of each.

| | 0.3.2 | 0.3.4 | Now | Game's own aiming |
| --- | --- | --- | --- | --- |
| Yaw after the view was pushed past its bottom or top limit eight times (1500 counts, 500 back) and once more | 0.0056 degrees per count, the controller at 85.8 degrees | 0.0762, whole in the first frame | the same | |
| Y after that | the view stays 71 degrees down or 44 up | levels it (7 down), the controller 3 degrees down | the same | |
| The same with `mouse_gyro_pitch = 45` | the controller at 41.8 degrees; Y leaves the view 38 degrees down | yaw 0.0762; Y levels it | not measured again | |
| Change of the game's pitch (-1 to 1) across a change of form | up to 0.35 | up to 0.003 at level and near the bottom, up to 0.008 near the top | up to 0.0009 | up to 0.003 |
| The human form's view at the start of every other cycle | within 1.9 to 4.4 degrees over 6 cycles | within 1.4 degrees near the bottom, 2.2 at level and 5.1 near the top over 17 | within 1.7 near the bottom, 0.4 at level and 0.5 near the top over 17 | within 0.1 degree |
| Steps after those changes of form, human form | yaw 0.0762; pitch 0.0761 to 0.0762 | yaw 0.0762; pitch 0.0758 to 0.0762 (0.5% at most, near the bottom) | yaw 0.0762; pitch 0.0759 to 0.0762 (0.4% at most, near the bottom) | |
| Steps in squid form | pitch down to 0.0686 near the top | yaw 0.0762; pitch 0.0761 to 0.0762 | yaw 0.0762; the squid camera's view 0.0663 to 0.0673 near the bottom, 0.0645 to 0.0652 at level, 0.0221 near the top (the human form's view moves by the counts) | |
| Squid form in ink: steps of 100 counts; pictures at 480 by 270 after 50 counts | | yaw 0.0762, pitch 0.0762 to 0.0763; the picture moves 15.0 px right and 16.7 px up (human form 16.8 and 16.2): the squid camera sits close to the floor that fills the middle of the picture | yaw 0.0762, pitch 0.0647 to 0.0650 (below level); the picture moves 15.0 px right and 4.5 to 4.7 px up (above level) | |
| Swimming up an inked wall | | each step turns the view by its counts in its own frame (7.47 to 7.75 degrees for 7.62); the climbing camera then turns the view by itself, up to 0.9 degrees of yaw and 0.7 of pitch over the next 10 frames, and 6.5 degrees of pitch in 200 frames without any count | yaw steps 7.52 to 7.62 degrees in their own frame, pitch steps 7.61 to 7.68 up and 6.43 and 7.17 down (those two while the camera was narrow or changed fast); the climbing camera's own turns over the 10 frames after a step as before | the same camera: 6.8 degrees of pitch in 200 frames without any count, 0.6 of yaw after a step |
| The sweep over the whole range, 25-count steps | | 0.0761 to 0.0763 from 75 degrees down to 58 up, also with `mouse_gyro_pitch = 45` | 0.0759 to 0.0765 from 71 degrees down to 58 up (the extremes in pairs of neighboring steps that make 0.0762 together) | |
| Circles of 250 counts (2 s to 0.33 s a turn) and 60 counts, up and down only | | pitch left after a straight fit at the yaw's gain: 0.01 to 0.47% | 0.01 to 0.52% | |

Release 0.3.4 also left the game's pitch up to 0.008 lower (about 0.5 degrees of view) after a change back to human form near the top with the mouse moving, always the same way in the bench's repeated pattern, so 17 changes moved the view there by 5 degrees; now 17 changes keep it within 0.5 degree. The steps in squid form and in ink are no longer the yaw's: that is the cost of keeping the human form's aim where the counts put it.

## Xbox button prompts

> Upstream targets only (*Tears of the Kingdom*): this target has none of it ([settings.md](settings.md), "The Esc
> menu on the Splatoon 3 target").

`KLM_PROMPTS` (`prompts`, the *Button prompts* row on the Controls tab, read at start) picks the controller whose buttons the game's prompts show: `nintendo` (default) keeps the game's own, `xbox` shows an Xbox controller's, `pc` the keyboard and mouse (PC button prompts, below).

- L, R, ZL, ZR, Plus and Minus become LB, RB, LT, RT, Menu and View, and the d-pad prompts a cross with an arrow for each direction meant. The letters on the keys are cut out in the game's own text font.
- A, B, X and Y name the label of the button the pad layout presses, so each prompt names the button to press. `pad_layout = nintendo` plays an Xbox pad by position: the right button, labeled B, does Switch A (interact, confirm), and the bottom one, labeled A, does Switch B (run, cancel), so the prompts swap A with B and X with Y. `xbox` and `auto` put Switch A on the button labeled A, and the letters stay.

| Switch | `nintendo` layout | `xbox` or `auto` layout |
|---|---|---|
| A, B, X, Y | B, A, Y, X | A, B, X, Y |
| L, R | LB, RB | LB, RB |
| ZL, ZR | LT, RT | LT, RT |
| Plus, Minus | Menu, View | Menu, View |
| D-pad | cross | cross |

The action prompt (`DoCommand_00`) shows its button in a diamond whose shape gives the face buttons' positions as on a Switch, with the other three letters around it. With `nintendo` its letters are swapped with the rest and every one names the button in that place. With `xbox` and `auto` the letters stay and the diamond is laid out as an Xbox pad: A at the bottom, B on the right, X on the left and Y at the top, the button the prompt names lit in its place, its bar reaching left from X and right from the others (`tools/make_romfs_patches.py`, `bflyt_xbox_diamond`: the panes of `blyt/DoCommand_00.bflyt` take their partner's place, A's of B and X's of Y). The sticks keep the Switch icons, and so do prompts drawn as fixed images (some tutorial animations).

The prompts are part of the game's fonts, textures and messages, so they follow the layout and not the bindings. A button remapped with a `bind_` line or on the Controls tab keeps the prompt the game draws for it, which then names the wrong button. Use the Xbox prompts with the default bindings.

The game reads its UI archive and fonts once at boot, so the prompts change at the next start, also after `pad_layout` changes between `nintendo` and the others (the menu says *prompts change after restart*); the buttons change at once. Under that note the menu offers *Restart now*: after a confirmation it writes `totk.ini`, closes the game as *Quit game* does and starts the game again with the same command line, which waits until the old process is gone before it opens the log and the saves (`runtime/gfx_present.c`, `gfx_window_closed`; `plat_restart` and `plat_restart_wait` in `runtime/platform/win32.c` and `linux.c`). Progress since the last save is lost, as with *Quit game*; while the game writes a save, the confirmation waits. With `prompts = xbox` the game loads `romfs-patches-xbox` (`pad_layout = nintendo`) or `romfs-patches-xbox-label` (`xbox`, `auto`) next to `romfs-index.bin` in place of `romfs-patches` (`runtime/romfs.c`, `overlay_init`; `runtime/input_map.c`, `input_prompts_folder`): the same 60 fps patches, with the prompts drawn from the game's own files by `tools/make_romfs_patches.py --prompts xbox` or `--prompts xbox-label` (`tools/button_icons.py`, after MacNativeGames' `make_button_icons.py`), which the setup runs next to the plain one:

- every prompt but the button guide in play is text in the button font, `scft/External.bfotf` in each `Font/Font*.Nin_NX_NVN.bfarc.zs`: glyphs U+E040 to U+E043 (A B X Y), U+E044 to U+E047 (L R ZL ZR), U+E04B and U+E04C (Plus, Minus), U+E049 and U+E060 to U+E065 (the d-pad). fontTools writes the font again, so the archives grow and the folder also carries the resource size table (`System/Resource/ResourceSizeTable.Product.<n>...rsizetable.zs`) with their entries raised by as much; without that the game stops at start.
- the button guide in play draws L and ZL from the images `Nt_KeyTexL_00` and `Nt_KeyTexZL_00` of the Common archive's `timg/__Combined.bntx`, painted with the same icons. The action prompt draws its button from `Nt_KeyTexA/B/X/Y_00`, whose image data is swapped for `nintendo`, and the diamond's letters are text the game takes from `LayoutMsg/DoCommand_00.msbt` of each language's `Mals` archive (and the layout's own text panes behind it), swapped the same way.

The Switch prompts stay, with a note under *Button prompts* and a `RomFS overlay: ...` line in the log, when the folder is missing (an install made by an older setup, or one where the setup could not prepare it and showed a warning; running the setup again on the install folder writes it), when it fails its check, with a game resolution of another shape than 16:9 that has no folder of its own (below), and when the `romfs_mods` folder replaces one of its files: `romfs-patches` then loads in its place, where two overlays replacing one file would stop the game. In a source build, `.venv-py314\Scripts\python tools\make_romfs_patches.py --version <ver> --prompts xbox` (and `--prompts xbox-label`) writes `build\<ver>\romfs-patches-xbox` (`-label`); it needs fontTools from `tools/requirements-py314.txt`. `--xbox-prompts` is the older name of `--prompts xbox`.

At a game resolution of another shape than 16:9 the game loads `romfs-patches-xbox-<w>x<h>` (or `romfs-patches-xbox-label-<w>x<h>`) in place of `romfs-patches-<w>x<h>`: the same prompts with that shape's UI layout, `make_romfs_patches.py --prompts xbox --aspect <w>x<h> --reuse <install>\romfs-patches-xbox --out <install>\romfs-patches-xbox-<w>x<h>`. The setup writes both for each of the fifteen sizes it lays the UI out for, as for the PC prompts below, so on a display of one of those shapes the game resolution follows the display with these prompts too. At another size, without that folder, the Switch prompts stay (*off at this game resolution*).

## PC button prompts

> Upstream targets only (*Tears of the Kingdom*): this target has none of it ([settings.md](settings.md), "The Esc
> menu on the Splatoon 3 target").

`prompts = pc` (*PC* on the *Button prompts* row) shows the key or mouse button the default keyboard bindings press each Switch button with, whatever the pad layout. The game loads `romfs-patches-pc` in place of `romfs-patches`, written by `tools/make_romfs_patches.py --prompts pc` (`button_icons.py`, `pc_icons`), which the setup runs next to the Xbox ones:

| Switch | Prompt |
|---|---|
| A, B, X, Y | E, Shift, Space, left click |
| L, R, ZL, ZR | Q, R, right click, F |
| Plus, Minus | Tab, M |
| D-pad | 1 (left), 2 (up), 3 (right), 4 (down), as a cross or the keys of the directions meant |
| Left stick, its press | W A S D (or the keys of the directions meant), Ctrl |
| Right stick, its press | the mouse, with an arrow for each direction meant, C |

- The glyphs keep the width of the game's own, so the right-aligned guides do not run into their text. A key is a rounded square with its label cut out in the game's own text fonts (Shift: an arrow over its name); a mouse button is a mouse with that button filled and the others hollow.
- The button guide in play draws every button from images of the Common archive (`Nt_KeyTexA/B/X/Y/L/ZL_00`, `Nt_KeyTexLSt_00` and `_01`, the stick tilted and pressed): all of them are painted with the same icons. The letters of the action prompt's diamond are one character of the Normal font each: E, an arrow for Shift, a bar for Space, L and R for the mouse buttons.
- Like the Xbox prompts they follow the defaults and not the bindings. `--binds <totk.ini>` draws the first key of each `bind_` line of that file instead, for a folder made by hand.
- At a game resolution of another shape than 16:9 the game loads `romfs-patches-pc-<w>x<h>` in place of `romfs-patches-<w>x<h>`: the same prompts with that shape's UI layout, `make_romfs_patches.py --prompts pc --aspect <w>x<h> --reuse <install>\romfs-patches-pc --out <install>\romfs-patches-pc-<w>x<h>`. The setup writes one for each of the fifteen sizes it lays the UI out for, so on a display of one of those shapes the game resolution follows the display with these prompts too. At another size, without that folder, the Switch prompts stay (*off at this game resolution*).

## Window keys

| Key | Gamepad | Action |
|---|---|---|
| F1 or Esc | Minus and Plus together | open or close the settings menu; the game gets no input while it is open |
| F10 | L3 + R3 (both stick clicks) | performance overlay: off, frame rate, full panel |
| F11 | | borderless fullscreen on or off, saved to `totk.ini` |

The Minus+Plus combo never reaches the game. In the menu the arrow keys, the D-pad or the left stick move, Enter or A selects, Q and E or L and R switch tabs, Esc or B goes back (Switch button names, as in the table above).

## Remapping

The *Controls* tab of the settings menu lists, under the button layout, every Switch button, the eight stick directions the keys drive (movement and camera) and the shake key, each with two key cells and a controller cell. Select a cell, press A or Enter, then press the new key, mouse button or controller button; Esc or five seconds without a press cancel, and a controller button cancels a key cell. Delete, Backspace or X clears the selected cell. A key another action has moves to the new one (the description names the action it left); a controller button swaps with the one it replaces. A remapped controller button is physical: from then on the face buttons (A B X Y) stay where they are in both layouts, until the buttons are put back as the layout in use has them or *Reset all buttons* is chosen. F1, F8, F10, F11 and Esc cannot be bound (the menu, the flight recorder and the window keep them). Punctuation keys are named by their place on a US keyboard; the menu shows the character the keyboard layout in use puts there.

The tab writes `totk.ini` lines, one per Switch button or stick direction: `bind_<button> = <entry>, <entry>, ...`. The button is `A B X Y L R ZL ZR PLUS MINUS LEFT UP RIGHT DOWN LCLICK RCLICK`, or a stick direction `LSTICK_LEFT LSTICK_UP LSTICK_RIGHT LSTICK_DOWN` (movement, W A S D by default) and `RSTICK_...` (camera, the arrow keys), or `SHAKE` (keys only, G by default). An entry is `pad:<name>` or `key:<name>`:

- pad names: `south east west north l r zl zr start back lstick rstick dpad_left dpad_up dpad_right dpad_down lpaddle1 rpaddle1 lpaddle2 rpaddle2` (`zl`, `zr` are the triggers; the paddles are the back buttons of an Xbox Elite, DualSense Edge or Steam Controller, upper pair 1, lower pair 2, and no Switch button has them by default)
- key names: `a` to `z`, `0` to `9`, `f2` to `f7`, `f9`, `f12`, `num0` to `num9`, `space tab enter backspace lshift rshift lctrl rctrl lalt capslock insert delete home end pageup pagedown up down left right`, `num_add num_sub num_mul num_div num_decimal`, `minus equals lbracket rbracket semicolon apostrophe comma period slash backslash grave`, `mouse_l mouse_r mouse_m mouse_x1 mouse_x2`, `wheel_up wheel_down` (the scroll wheel, away from you and towards you: each notch is one short press of the button, so it suits a press like the jump and not a hold like shooting; on the map the wheel zooms instead. The Controls tab shows such a binding but cannot capture the wheel: write the line, or bind it in the launcher)
- `key:default` and `pad:default` keep that part's defaults (`pad:default` is each layout's own button).
- `none` leaves the button unbound.

A line replaces every default of that button, pad and keyboard, so list the keys you still want or use `key:default`. Pad names are physical positions, so a line means the same in both layouts and replaces the layout's own button for that Switch button. Stick directions take keys only: the controller's sticks and the mouse camera always work. Buttons without a line keep the table above, less any key a line binds: with `bind_X = key:t`, T presses X only, not also Y. Names are case-insensitive. An invalid entry logs one `input: bind ...` line and is skipped; a line without a valid entry changes nothing. The `KLM_BIND_<button>` environment variables work like any other setting. A changed line applies at once (`runtime/input_win.c` builds the tables again at its next poll).

A pad that reports its face buttons swapped (A/B and X/Y, as some GuliKit models do) is fixed with:

```
bind_A = pad:south, key:E
bind_B = pad:east, key:lshift
bind_X = pad:west, key:space
bind_Y = pad:north, key:mouse_l
```

## Mouse camera

> Upstream targets only (*Tears of the Kingdom*): this target has none of it ([settings.md](settings.md), "The Esc
> menu on the Splatoon 3 target").

`mouse_mode = direct` (the default) turns the camera by the raw mouse motion on games 1.4.3 and 1.2.1, through hooks in the game's camera code (`runtime/mouse_direct.c`, the `mouse_direct` sites of `recomp/patches-v143.txt` and `recomp/patches-v121.txt`). None of them changes a step of the gamepad alone. The free-orbit camera receives raw-count
angular increments at its final spherical-angle output: 0.08 degrees per count on both axes at sensitivity 1 (about
14.3 cm a turn at 800 DPI), with no time multiplier, curve or carry into later frames. The controller's persistent pitch/yaw move with
the output. Mouse input is removed from its stick path first, so the camera's later vertical signed-square curve and
velocity filters cannot process it a second time.
The aiming camera (bow, throwing), the Purah Pad camera, the scope, the ability camera (Ultrahand, Fuse, Ascend,
Recall, Autobuild), the horse camera, lock-on, the bow drawn while locked on, climbing (walls and objects), the
waterfall's turn, shield surfing, the wakeboard and the dialogue cameras take the mouse the same way, at the same
degrees per count: each turns by
the stick times its own speeds and the frame time and adds that to a base angle, so the load of the stick it turns by
gets the gamepad's part only and the load of that base angle gets the mouse's degrees (`camera_load`, two sites per
axis per camera). Their speeds, time step, slow motion (bullet time) and, for the Purah Pad camera and the abilities,
the eased curve their pitch moves along do not touch the mouse; their own limits, collision and blends apply to the
result. Past the pitch limit of the scope or the ability camera the stick moves the camera's distance; the mouse's
motion past the limit keeps doing that through the stick. Recall's cursor is the ability camera's offset mode: the
camera keeps a direction, the stick moves the cursor across the screen (ReverseRecorderCursorSpeed, within
ReverseRecorderCursorRange) and the camera follows it by up to ReverseRecorderCameraRange (10 degrees across, 7.8 up).
The mouse moves the cursor so that its aim, the camera's follow plus the cursor's angle from the screen's center (the
offset taken as the screen's half width or height at the camera's 50 degree view and aspect ratio), turns as many
degrees a count as the camera does elsewhere. The follow range is read from the game each step (until it has been read
once, the mouse is stick input there). The stick loads that only test for input keep the mouse in the stick. A step with mouse motion that no
direct hook took is logged (`mouse: step N with mouse motion took the stick path`, the first 30, then one in 600, with
the player's state), so the cameras still on stick input show in `logs/totk.log`.
Lock-on keeps pointing the camera at the target; the stick, and so the mouse, adds an offset around that direction,
which the game limits and lets return: the mouse's degrees are the step's turn the game adds to the offset and judges
the return by.
Shield surfing and the wakeboard turn their pitch by the square of the stick's y and move a second camera parameter
with it: their stick y keeps the mouse (that parameter and the camera's state follow it as before), the pitch gets
the degrees, and the pitch turn the game made from y is cut to the gamepad's share (p |p| / t |t|). Their stick x, and
the dialogue camera's (which is also its test for input), give a tiny value in the mouse's direction when the
gamepad's axis is at rest, so the camera still sees input without a stick turn.
Two inputs stay stick input: the waterfall's vertical axis moves the camera's height, not an angle, and the horse's
lock-on without a target (HorseLockOnEmpty) turns the stick's position into an offset that returns when the stick
does, which a mouse has no position for.
Those receive input after
the radial dead zone, unit-circle clamp and stick response exponent: x is 0.0343 stick units a count over the frame
ratio (the stick turns yaw 70 x / 30 degrees a step) and y 70 / 51 times that, linear like their pitch (51 y / 30
degrees a step), so both axes turn by the same amount for the same motion. `stick_sens` (which scales the game's
stick turn rates) is divided out. While the player uses an ability and the ability camera's hooks do not take the
mouse, its part is scaled by 1 / 0.8 and 70 / (1.5 x 51), the ability camera's own stick scales.
The player's verbs are read every 50 ms from the player actor (the player root's GOT slot main+0x3969918, the
component array +0x228, its AS component +0x158 and the verbs of its five slots). Ultrahand has a separate hook after
its manipulation input's power curves; it gets 0.01 stick units a count over the frame ratio, at most three full tilts
a step.
`mouse_aim_scale` (0.1 to 3, default 1) multiplies the mouse's motion while the bow is drawn (the verb UseBow or
DrawnBow with ZR held). The game's camera inversion and gamepad response remain in effect. `mouse_sens` scales raw
counts, with the same meaning as in 0.3.10 (the free camera turns 0.08 degrees a count at 1, and now every other camera
does too); its scale differs from the stick-emulation mode.
The Controls tab switches between *Direct* and *Stick emulation* (`mouse_mode = stick`).
The orbit camera keeps its desired pitch at the final angular output, including stationary frames, so a later game
filter cannot recenter it. The persistent filter state accounts for the camera's current vertical offset instead of
accumulating corrections. A gamepad vertical input releases that mouse target to the game's own. The game's
lock-on, recenter and override (aiming) drive the camera themselves: the mouse stops holding without restoring its
earlier target, and when they end (or after the camera went 100 ms without a step) it holds the pitch the camera has
then, so the camera does not spring back to a pitch from before the aim or to the game's default after a shot.
While the ability wheel is open (L held and its screen, ShortCutMainItem_00 or ShortCut_00, has the focus), mouse
counts move its virtual cursor, which stays where it was when the mouse stops. L or the D-pad held with no wheel or
strip on screen (L soothes the horse, turns menu tabs) leaves the mouse on the camera; with the menu cursor off
(`menu_cursor = 0`) the screens are not read and the buttons alone decide.
The horizontal quick lists (D-pad up/left/right: materials, shields and weapons) use separate 32 ms right-stick steps,
with at least 32 ms of neutral between them. At sensitivity 1, 32 horizontal counts request a step; a large flick
cannot queue continued scrolling after the mouse stops. Vertical movement is ignored in these lists. Switching or
releasing a selector resets its cursor/step state without sending the last movement to the camera. Selector input is
not consumed by the direct camera or Ultrahand hooks.
To detach a fused part from what Ultrahand holds, shake the mouse left and right, or press G (`bind_SHAKE`). A press
swings the right stick left and right for 0.8 s, holding the key keeps it swinging. With `mouse_mode = direct` a shake
of the mouse while Ultrahand holds an object swings the right stick with it: once the swings (10 degrees of camera turn
or more each, 125 counts at sensitivity 1) change direction twice within 0.35 s, the stick goes full tilt the
way the mouse moves, each side held at least two frames so the game reads it, and goes neutral as soon as the mouse
rests. Only your own swings reach the stick, so nothing keeps swinging after you stop; shake until the part comes off.
The camera hooks ignore that swing (the mouse turns the camera in degrees already). Turning a held object back and
forth at an ordinary pace is slower and does not count. The arrow keys or the gamepad's right stick shaken by hand
work too.
A step of the gamepad alone, with no mouse motion and no pitch the mouse holds, reaches the camera exactly as without
the hooks.
The mapping below is still used when the camera-input hook is unavailable or the game disables camera input.
Game 1.1.0 has no hooks and keeps stick emulation whatever `mouse_mode` says. On 1.2.1 `mouse_aim_scale` has no effect (the bow-in-hand state is not read there).

In stick-emulation mode, mouse motion (raw input) becomes right-stick deflection. Each presented frame takes the counts moved since the last frame, divides by the frame time and sets the deflection to `counts per second * KLM_MOUSE_SENS / 3000` of full, held for that frame, so the turn rate does not depend on the frame rate. This is not mouselook: the game's stick curve and dead zone still apply, so very slow movements do nothing and a fast flick saturates. The mouse deflection competes with the gamepad and arrow keys by the larger-deflection rule.

`KLM_MOUSE_SENS` (0 to 10, default 1; higher turns faster, 0 turns the mouse camera and the cursor capture off). While the window is the foreground window the cursor is hidden and confined to the client area. It is released on focus loss, minimize, while F11 changes the window, during a size or move loop and while a text entry is open. Esc releases it by hand: the cursor shows, the mouse camera and mouse buttons stop, and the window can be moved, minimized or closed. A mouse click inside the client area captures it again; that click does not reach the game. Activating the window by a click on its title bar or border does not capture, so it can be dragged or closed right away; Alt+Tab or a click in the client area captures as before. `KLM_INPUT=none` disables the capture too.

`KLM_STICK_DEADZONE` (0 to 50, default 8) is the gamepad stick dead zone in percent of full deflection, the same for both sticks. It is round: a stick inside the circle reads centered, and the rest of its travel is scaled to start at 0 at the edge, so full deflection still reaches full. Raise it for a stick that drifts. The game's own dead zone applies after it. `KLM_LSTICK_DEADZONE` and `KLM_RSTICK_DEADZONE` (the *Left/Right stick dead zone* rows under *Analog sticks* on the Controls tab) set one stick each and fall back to `KLM_STICK_DEADZONE` while unset, so a worn stick can get a larger zone than the other. With a gamepad connected each row shows its stick's deflection now, before the dead zone (*stick now*): let go of the stick and set the row a few percent above the value it rests at.

The stick direction bits (`StickLLeft` to `StickRDown`) are derived from the sticks at 50% deflection. A gamepad plugged in later is picked up within one second; with none connected the log says `input: no gamepad`.

## Mouse in the game's menus

> Upstream targets only (*Tears of the Kingdom*): this target has none of it ([settings.md](settings.md), "The Esc
> menu on the Splatoon 3 target").

On game 1.4.3, while one of the game's menus has its focus box (the frame the d-pad moves: the title, the save list, the inventory and its System tab, Options, shops, dialogue choices), the cursor is released from the mouse camera and shown in the middle of the window. A focus gap of up to 300 ms (a tab switch, a popup opening: the game drops the focus box for a frame or two) keeps the pointer where it is instead of capturing and centring it again. The item under the pointer takes the focus as if the d-pad had moved there, with the game's highlight, sound and description; a left click is A on it, a right click is B. The middle and side mouse buttons keep their bindings (bound to L and R they turn the tabs), and T presses Y (sort). The keys and the pad keep working, and the focus moves only when the pointer moves. This happens only while the keyboard and mouse are the device used last: after a gamepad button press or a stick pushed past half way the menus are as on the console, with no pointer, until the mouse moves or a key is pressed (`input_kbm_last` in `runtime/input.c`). The ability wheel, the d-pad strips, Autobuild and the Purah Pad's camera stay gameplay: the mouse keeps the camera there. The map has no focus box; it is found in `eui::ScreenMgr`'s list of open screens (every 100 ms: the Purah Pad's frame `AppMenuOverlay_00` with `AppMap_00` and the menu background `AppMenuBG_00`), and there the mouse moves the map's cursor as the left stick does, with the cursor still captured, and the scroll wheel zooms: each notch is a full-tilt push of the right stick (the map's zoom) for 100 ms with 50 ms of neutral after it, up (in) for a turn away from you; a fast scroll's notches wait their turn (at most four), a turn the other way drops them. The Scope opens the same map screen without the menu background and keeps the mouse camera. When the menu closes the cursor is captured again.

`runtime/menu_cursor.c` reads eui's screens at each pad read and moves the focus with the writes `eui::BoxCursorMgr::moveBoxCursor` makes, so no game code is called. `menu_cursor = 0` (`KLM_MENU_CURSOR=0`, live; the *Mouse in the game's menus* row of the Controls tab) turns it off; `KLM_MENU_CURSOR_LOG=1` logs every screen it reads (by default the first six), with the layout size, the node count and the hit panes it uses.

## Nintendo Switch 2 controllers

Windows does not present the Switch 2 Pro Controller as a gamepad SDL can open (the log says `input: no gamepad`), so it needs a bridge program that wakes it and creates a virtual gamepad. A bridge whose output is an emulated DualSense gives buttons, rumble, gyro and accelerometer: the game reads the virtual DualSense directly, and the log then shows `input: gamepad "DualSense Wireless Controller" (rumble, gyro and accelerometer, ...)` and `input: motion sensors on`. A bridge whose output is a virtual DualShock 4 may give buttons and rumble but no motion (a gyro and an accelerometer that never change). Such bridges are third-party programs, not tested or endorsed by this project. Run one bridge at a time; Steam Input can also claim the controller, so close Steam if the bridge does not find it.

## Scripted input

`KLM_INPUT_SCRIPT=<file>` replays presses. It is ORed with real input and still applies under `KLM_INPUT=none`. One event per line, `#` starts a comment:

```
<frame> <button> [hold_frames]            # A B X Y L R ZL ZR PLUS MINUS LEFT UP RIGHT DOWN LCLICK RCLICK; hold defaults to 4
<frame> LSTICK|RSTICK <x> <y> [hold_frames]   # -32767..32767, up is +y
<frame> MOUSE <dx> <dy>                  # direct-mode raw counts, once; down is +dy
@<ms> <button> [hold_ms]                  # timed on the world clock
@<ms> LSTICK|RSTICK <x> <y> [hold_ms]
@<ms> MOUSE <dx> <dy>
```

`<frame>` counts 60 Hz ticks of game time since the first frame (presented frames at 60 fps), so the title menus get the presses at the same game time at any frame limit. `@<ms>` counts milliseconds of the world clock: game time stepped since the end of the first save load (the player leaves the title screen's origin), so a timed event lands at the same game time at any frame rate and load length. A later load, such as fast travel, still shifts what follows. In a script with timed lines the untimed ones stop when the world clock starts, as the load ends at another tick in each run. Each event logs once (`input: script frame N tick T: A`). `tools/input/load_save.txt` loads the first save from the title screen.

`KLM_INPUT_RECORD=<file>` (`input_record` in `totk.ini`, or Input recording in the menu) writes the merged input in the same format, one line per button press and per stick segment: events before the world clock starts by tick, the rest timed; a button or stick held when the world clock starts is cut there and goes on as a timed event. The file replays as a script, which makes it the repro to attach to a bug report.

In direct mouse mode the recording also writes one `MOUSE` event per consumed raw displacement. These are one-shot
displacements rather than held stick segments. They replay without cursor capture, including with `input = none`;
the game must be accepting camera input when the event reaches its camera update.

`KLM_TEST_PAD=<lx>,<ly>,<rx>,<ry>` attaches an SDL virtual gamepad at start whose sticks rest at these raw SDL axis values (-32768 to 32767, down is +y), so a drifting or held stick can be tested without a controller: the values go through the gamepad mapping and `KLM_STICK_DEADZONE` like a real pad's, and `KLM_INPUT_RECORD` shows what reached the game.

## RenderDoc capture

RenderDoc's Vulkan layer (1.47) hides `VK_EXT_external_memory_host`, which the back end imports the game's CPU-visible
pools with, and its capture option `AllowUnsupportedVendorExtensions` does not bring it back. `KLM_RDOC` turns on
capture mode (`runtime/rdoc.c`): those pools get shadow storage (`runtime/vk_memory.c`, the same path as `KLM_SHADOW=1`)
and the device is chosen without the extension. Start the game under RenderDoc, which injects itself:

```
renderdoccmd capture -w -d <working dir> -c <dir>\splat3 splat3.exe      (with KLM_RDOC set in the environment)
```

`KLM_RDOC=<n>,w<s>,...` captures the n-th presented frame and the first frame presented after the world clock passed
s seconds; `boot` in the list captures the work before the first present, from the device's creation on (textures the
game builds once at boot, such as Splatoon 3's menu environment cube); any value captures on RenderDoc's F12 and Print
Screen keys. `KLM_RDOC_DIR` names the captures' folder.
The game looks for a RenderDoc already in the process and never loads it; without one, capture mode only changes the
memory path, which is how its pictures can be compared with a normal run's. RenderDoc also hides
`VK_EXT_device_fault` and `VK_EXT_device_address_binding_report` (no fault report under it) and NGX's
`VK_NVX_binary_import` and `VK_NVX_image_view_handle` (no DLAA).
