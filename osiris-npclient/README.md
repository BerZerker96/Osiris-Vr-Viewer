# Osiris TrackIR bridge (`NPClient64.dll`)

This folder builds the small **client DLL** that TrackIR / NPClient games load to
read head-tracking data. It lets Osiris drive native TrackIR games **without
OpenTrack installed**.

## Why this exists

The Osiris viewer already writes your head pose into the FreeTrack shared-memory
block (`FT_SharedMem`) whenever the **TrackIR Game** toggle is on. But games
don't read that block directly — every TrackIR/NPClient game loads a *client
DLL* (`NPClient64.dll`) and calls its `NP_GetData` function. This DLL is that
bridge: it reads `FT_SharedMem` and hands the pose to the game.

```
Osiris viewer  --writes-->  FT_SharedMem  --read by-->  NPClient64.dll  -->  game
```

The viewer needs **no changes** — this DLL is the missing piece between the data
it already publishes and the game. (This is exactly what OpenTrack's own
`NPClient64.dll` does; here you build your own so you don't need OpenTrack.)

## Build it

You need the MSVC C++ build tools — you already have them if you build the
viewer (the Rust MSVC toolchain ships the VS Build Tools with `cl.exe`).

Just **double-click `build-npclient.bat`**. It finds the Visual Studio C++
tools and sets them up itself, then builds **`NPClient64.dll`** in this folder.
The window stays open at the end so you can read the result.

```bat
build-npclient.bat        :: 64-bit NPClient64.dll  (ES2 + most modern games)
build-npclient.bat 32     :: 32-bit NPClient.dll    (older 32-bit games)
```

(You can still run it from an "x64 Native Tools Command Prompt for VS" if you
prefer — it detects that the compiler is already set up and skips straight to
building. For a 32-bit game, use `build-npclient.bat 32`.)

If it reports it can't find `cl.exe`, install Visual Studio or the standalone
**"Build Tools for Visual Studio"** with the **"Desktop development with C++"**
workload (the same toolset the viewer build needs), then run it again.

> No C compiler / don't want to build it? Any FreeTrack-compatible
> `NPClient64.dll` works the same way — but building this one keeps everything
> in-house and ISC/MIT-clean.

## Install it — two cases

**Most TrackIR games (DCS, Elite, ETS2, Assetto, Falcon BMS, ARMA…)** look up
the DLL through a registry key. Put `NPClient64.dll` (and `NPClient.dll` for
32-bit games) in a folder you keep — e.g. `C:\Osiris\trackir\` — and point the
registry at it:

```
HKEY_CURRENT_USER\Software\NaturalPoint\NATURALPOINT\NPClient Location
    Path  (String)  =  C:\Osiris\trackir
```

Create that key/value once with `regedit` (or a `.reg` file). The game then
loads your DLL from that folder.

**Everspace 2 (and other Unreal-plugin TrackIR games)** ignore the registry and
load the copy bundled **inside the game folder**. Replace that one:

1. Go to the game's TrackIR plugin folder, e.g.
   `…\steamapps\common\EVERSPACE™ 2\ES2\Plugins\TrackIR\…\NPClient\Win64\`
2. **Rename the existing `NPClient64.dll`** to `NPClient64.dll.bak` (keep it!).
3. Copy **this** `NPClient64.dll` in its place.

Then in Osiris turn on **TrackIR Game**, and in the game enable **TrackIR** in
its control/gameplay options. If an axis points the wrong way, flip it with
Osiris's per-axis controls (pitch inversion is common and expected).

## How it maps the data

Rotation `radians × 16383 / π`, position `mm × 16383 / 500`, clamped to
±16383 — identical to OpenTrack, so axes behave the same way they do for
OpenTrack users.

## Provenance & licence

Ported essentially verbatim from OpenTrack's `contrib/npclient/npclient.c`,
which is distributed under the permissive **ISC licence** — deliberately *not*
the GPL-derived FreeTrack translation, so it can ship alongside Osiris (MIT).
The NaturalPoint interface signature it returns is reproduced **only for
interoperability** (so games accept a non-NaturalPoint tracker), exactly as
OpenTrack, FreeTrack, FaceTrackNoIR and linuxtrack all do. Credit to the
OpenTrack project (https://github.com/opentrack/opentrack).
