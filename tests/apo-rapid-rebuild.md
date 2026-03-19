# APO Rapid Rebuild Issue

## Problem
When applications like YY Voice (YY語音) frequently create/destroy audio sessions
(e.g., users joining/leaving voice channels, channel switching), Windows audio
engine (audiodg.exe) rebuilds the audio graph, causing the APO to be destroyed
and recreated in rapid succession.

## Symptoms
- Continuous stuttering ("卡卡卡卡") when YY Voice is active with many users
- Not the same as DPC spike (which causes brief mute + recovery)
- Audio cuts out repeatedly as APO reinitializes

## Log Evidence
```
LockForProcess called → frame_count=9  → unlock → dropping  (only 90ms alive)
LockForProcess called → frame_count=3  → unlock → dropping  (only 30ms alive)
LockForProcess called → frame_count=2396 → unlock → dropping
```

Normal operation should show frame_count in the tens of thousands (minutes of
continuous processing). frame_count < 100 indicates the APO was destroyed almost
immediately after creation.

## Root Cause
YY Voice opens 17+ yyexternal.exe sub-processes. Each voice channel/user may
trigger audio session creation/destruction, which causes audiodg.exe to rebuild
the entire audio graph including all APOs.

Each rebuild cycle:
1. APO destroyed (UnlockForProcess + Release)
2. APO recreated (CreateInstance + Initialize + LockForProcess)
3. Engine re-initialized (FFT plans, OLA buffers, prime with silence)
4. Click gate armed → fade-in from silence
5. Only processes a few frames before being destroyed again

## Impact
- Each rebuild introduces ~50ms of silence (prime + click gate recovery)
- At 10+ rebuilds per second, this creates continuous stuttering
- Compounded by DPC spikes from Docker/Hyper-V running simultaneously

## Potential Solutions

### 1. Engine state persistence across rebuilds
Cache the engine state (OLA buffers, last output samples) in a global static
so that when the APO is recreated, it can resume from the previous state instead
of re-priming from silence. This would eliminate the click gate fade-in on rebuild.

### 2. Debounce rapid LockForProcess calls
If LockForProcess is called within N ms of a previous UnlockForProcess with the
same format (sr/ch/frame_size), skip the full re-initialization and reuse the
existing engine state.

### 3. STORED_ENGINES reuse
The STORED_ENGINES global already exists for engine reuse. Verify that it works
correctly during rapid rebuild cycles — the engine should be retrieved from storage
instead of being rebuilt from scratch.

## Reproduction
1. Install Phaselith APO
2. Open YY Voice and join a busy channel (10+ users)
3. Play music simultaneously (YouTube/Spotify)
4. Observe stuttering in music playback
5. Check apo_debug.log for rapid LockForProcess/unlock cycles

## Priority
HIGH — This affects any user running voice chat software (YY, Discord, Teams)
alongside music playback. Voice chat is a common use case.
