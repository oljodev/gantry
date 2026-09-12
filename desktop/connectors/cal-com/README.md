# Cal.com

Cal.com's hosted server: bookings, event types and availability.

- **Runs:** nothing locally. `https://mcp.cal.com/mcp`.
- **Needs:** a Cal.com account; sign-in happens in your browser and the server registers Gantry on the spot.
- **Can reach:** your bookings and event types. **Cancelling or creating a booking sends mail to the other person**, which is the "messages a human" rule of 17 §6: it is confirmed whatever the mode.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
