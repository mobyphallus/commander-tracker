# Phone feedback

Mark a player out, then tap **Rate this game** on their tile. Scan the QR code on
that player's phone, on the same Wi-Fi or local network as the laptop. Keep
Commander Pod open while people submit feedback. Opening the link does not pause
the game timers.

The form records a 1–5 enjoyment rating, optional problem-player and kingmaker
selections from the match's participants, and up to 2,000 characters of notes.
Feedback is linked to the submitting player and match. The laptop owner can read
it under **Game History → match → Player feedback**. Use **Refresh** there to
load new submissions. History also offers personal feedback links for eliminated
players after the match ends.

A player can revise their response using the same link; repeated submissions
update one response rather than adding votes. Bringing a player back in disables
their link until they are marked out again. Undo and recovery preserve the link
and existing responses. Abandoning a game disables its links; collected responses
remain in the database marked as belonging to an abandoned match. New matches
get different links.

Responses live in SQLite and are included in normal backup/restore. Pending
responses attach to the final game record in the same transaction as saving the
result. Player impressions are displayed separately from recorded game outcomes.

The app serves HTTP on IPv4 port **8787** while open. It does not configure router
port forwarding or a public website. Each link contains a random 128-bit token
that allows editing only that player's response; anyone given that link can use
it. The phone endpoint never lists other responses. Old matches played before
this feature do not have feedback links.

QR rendering uses `qrencode` when available; the full URL is also displayed.
If the page cannot connect, check that both devices are on the same network and
that the network/firewall permits connections to the laptop on port 8787. A
port-in-use error is shown in the QR panel; close the other app using that port
and reopen Commander Pod. No game data needs to be discarded.
