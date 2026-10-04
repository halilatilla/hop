# Hop

Hop sends the Bluetooth devices you choose from this Mac to your other Mac. That can be a mouse, a keyboard, headphones, a trackpad, or anything else already paired. Each device stays paired to both Macs and connected to one. You pick which ones move.

The two Macs already know each other. They use the same Apple Account, they are on the same network, and the devices are already paired in Bluetooth on both. Hop does not pair them, and it does not introduce the Macs.

## How a move works

A Bluetooth device such as a Magic Mouse or a pair of headphones connects to one Mac at a time, and it can remember more than one. Pair each device in System Settings → Bluetooth on both Macs once. After that, Hop does not ask you to pair again.

Hop runs in the menu bar on both Macs. **Sending** disconnects the devices you chose on this Mac, then the other Mac connects them. That is `IOBluetoothDevice` `closeConnection` here and `openConnection` on the other Mac.

The other Mac has to be awake and running Hop. If it is asleep, the devices have nowhere to land.

## What this build does

The window lists every device paired in Bluetooth. Tap one to include it. The choice is saved at:

`~/Library/Application Support/Hop/chosen.txt`

One address per line. Hop reads the list again every couple of seconds, so a device you pair while the window is open shows up.

The link between the two Macs is not in this build yet. Hop will not disconnect a device until the other Mac can take it. A button that disconnected your mouse or headphones first would leave them with nowhere to connect.

macOS asks for Bluetooth access so Hop can read the paired devices. Local Network access comes when the two Macs link.
