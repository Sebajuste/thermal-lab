# Thermal Lab

**Cut your PC's power draw, and measure what it actually costs you.** A Windows tray app
that reads temperature, power and clock speed on both sides of the switch.

## Why

Commercial "thermal optimizers" do one thing: they disable Turbo Boost through a Windows
power setting. On an i9-14900K that takes the CPU from ~5.7 GHz and 250-350 W down to its
3.2 GHz base clock at 80-100 W.

In games — where the GPU is usually the limit — the frames-per-second cost is marginal.
The drop in heat and power is not.

Thermal Lab gives you that same switch, and measures both sides of it. You see the trade
instead of taking someone's word for it.

## What it does

- Live CPU and GPU readings at 1 Hz: temperature, power, clock, load
- One switch to cap the CPU on your active power plan, with a choice of profile:
  *Light* turns the turbo off, *Aggressive* also caps the clock and leans Speed Shift
  toward saving power
- An opt-in cap for a graphics card that draws power while doing nothing — no display,
  no load, yet stuck in a performance state. Released at the first sign of work
- A one-click diagnostic in the System tab: every CPU setting as each source sees it,
  the restore journal and the GPU state, on one screen you can photograph
- A compare tab that averages each state separately — flip mid-game, read the difference
  at the same workload
- Lives in the notification area; left-click the icon for the panel, right-click for the
  menu

## Install

Grab the installer from [Releases](../../releases). It updates itself from then on —
on a button, or unattended once *Update automatically* is ticked in the System tab.

Windows 10 or 11, 64-bit. Changing a power plan needs administrator rights, so the app
asks for elevation at launch. Decline it and everything still runs — read-only, with the
switch disabled and the reason shown.

## Temperatures need a helper

Reading CPU die temperature requires a kernel driver. Thermal Lab ships none — the usual
candidate is on Microsoft's blocked-driver list — and reads one you already trust instead:

**[Core Temp](https://www.alcpu.com/CoreTemp/)**, **[HWiNFO](https://www.hwinfo.com/)**,
or **[LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)**,
running as administrator. LibreHardwareMonitor also covers Radeon GPUs; NVIDIA cards are
read directly from the driver.

Without any of them, everything else still works. Per-core frequency alone shows whether
the turbo is capped — it is the more reliable indicator anyway.

The System tab tells you what each source provides, whether it is running, and what to do
about it.

## Good to know

- Quitting puts your power plan back exactly as it was before Thermal Lab touched it —
  every exit, and that includes a crash, a killed task or a Windows shutdown. What it
  cannot undo on the spot is written down and undone at the next launch. The cap lasts as
  long as the app runs, and nothing of it is left on your machine afterwards.
- The cap applies to the **active** power plan. Switching plans in Windows changes the
  target; the current plan name is always on screen.
- No frames-per-second measurement — that would need an overlay, which is out of scope.
- On a managed work machine, expect read-only: installing a monitoring tool and editing
  power plans both need rights you likely don't have.

## License

[MIT](LICENSE). Built with [Tauri](https://tauri.app).

---

Developer documentation — architecture, sensor providers, measurement method, release
process — lives in [docs/](docs/README.md), in French.
