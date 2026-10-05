# Performance audit

Measured on Hop 0.2.0, Apple silicon, macOS 26.6.2, 5 October 2026. The menu was closed. No move was started. Whether the other Mac was running Hop was not checked. A live move was not run, because that disconnects a device this Mac is using.

Wakeup counts were not measured. `powermetrics` asked for a password and was not run. Bluetooth enumeration time was not timed inside the process.

## What the 250ms tick does

`Hop::start` waits 250ms, then runs `on_tick` on the UI thread.

Every tick:

- Take any share or removal the other Mac already sent. If the queues are empty, this returns.
- If a move is in flight, see whether that thread has finished or has run for 90 seconds. If nothing is moving, this returns.
- Decide whether Bluetooth is due.
- Copy the paired-address set and the shared-address set, and send them to the link only when they differ. Building the paired set still happens every tick.
- Read whether the other Mac is visible. If that changes, update the menu.
- Every 8 seconds, while the other Mac is connected, send the shared list again.
- Drain menu clicks.

Bluetooth is not read on every tick. `pairedDevices` runs on the UI thread:

- every 8 seconds while idle (32 ticks)
- every 1 second while a move is running, and for 10 seconds after it finishes
- once when the move finishes

The other Mac is not found on this timer. `src/link.rs` blocks in `poll` on the Bonjour sockets and the listen socket, with a 200ms timeout, on its own thread. A packet wakes that thread sooner. The timeout also wakes it when nothing arrived.

A menu click is queued by AppKit and applied on the next tick, so the 250ms wait is the worst delay before **Move here** starts. The same tick is what clears **Moving…** after the move thread finishes.

## Idle

`ps -p <pid> -o pcpu=,rss=` every 2 seconds for 40 seconds, process already running for about 7 minutes:

- CPU: 0.0% on 19 samples, 0.1% on one sample
- RSS: 70,272–70,576 KB

`sample` for 5 seconds, then again for 12 seconds, 1ms interval:

- Physical footprint about 55 MB (peak 56 MB)
- Main thread: 10,060 of 10,068 samples in the 12 second run were `mach_msg2_trap`, inside the AppKit event loop
- The 12 second call graph did not include IOBluetooth
- The 5 second call graph spent 6 samples looking up an SF Symbol and the rest waiting

The 250ms tick and the 8 second Bluetooth read did not show up as time on the CPU. The link thread's 200ms `poll` is a periodic wake. Its cost was not counted separately.

## Active move

Not measured on the hardware.

From the code, a **Move here** click starts one background thread (`link::claim`). The thread:

- sends `share`, then `want`, and waits up to 20 seconds for the reply
- on `released`, connects the device here and replies `took` or `failed`

The Mac that had the device disconnects only after it accepts the request. If the release fails, it connects the device again and replies `still`. If it released the device and does not see `took` within 70 seconds, it connects the device again.

The UI gives up after 90 seconds and says the devices stayed here.

While the move runs, the tick checks the thread every 250ms and reads Bluetooth every 1 second. That check is what the menu needs in order to leave **Moving…** without waiting for the idle 8 second read.

## Decision

Keep the current timers.

Idle CPU is already at the noise floor. The 250ms tick is the click and the end of a move. Bluetooth enumeration is already off that cadence. Stretching the tick would make **Move here** and the end of **Moving…** wait longer, and the samples do not show a cost that pays for that. Building a new paired-address set on every tick did not show up in the samples either.
