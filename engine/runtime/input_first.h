// Input first for Splatoon 3 11.3.0 (TOTK_INPUT_FIRST, input_first.c): the controllers are read before the game logic
// of the same frame instead of after it, so a frame's picture shows the input read in that frame.
#pragma once

// 1 while the frame's controller update runs before its logic: the patch set is in the build and on, and the setting
// is on. Read once per frame by the hooks and by the late-input gate (nvn_api_obj.c).
int input_first_active(void);
