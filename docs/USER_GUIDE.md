# Spaceshop Companion — Contractor User Guide

A short guide. If anything here is wrong or confusing, tell your project
lead so they can fix it.

## Installing

1. Double-click the `Spaceshop Companion ...msi` your project lead sent.
2. Windows may show a blue **"Windows protected your PC"** screen.
   This is expected. Click **More info** → **Run anyway**.
3. Companion installs to your user folder (no admin needed for the
   install itself). It opens automatically when done.

## First-time setup

1. Companion opens with a text box asking for your invite code.
2. Paste the invite (or click the link your project lead sent, which
   pre-fills it).
3. Click **Continue**.
4. Review the project name. Pick a folder where the project files should
   live — the default is fine in most cases. Click **Choose folder…**
   to override.
5. Click **Connect**.
6. **Windows will ask permission once.** Click **Yes**. This lets
   Companion set up the connection service.
7. Wait 2–5 minutes. You'll see progress steps fill in: parsing,
   preparing connection, joining the network, reaching the server,
   saving your access pass, setting up your workspace, downloading
   your project.

When done, you'll see a big "**You're connected**" screen with three
values to copy into Unreal.

## Pointing Unreal at the project

Open the project's `.uproject` file in Unreal Engine.

1. **Editor Preferences → Source Control**.
2. Set **Provider** to **Perforce**.
3. In Companion, click the **Copy** button next to **Server** and paste
   it into Unreal's Server field.
4. Same for **User name** and **Workspace**.
5. Click **Accept Settings** in Unreal. The indicator should turn green.

You're done. Use Unreal's normal source-control menu to check out, edit,
and check in assets. Companion can stay running in the tray (bottom-right
corner of the screen) — it doesn't need to do anything else.

## Things to know

- **You don't have a password.** Your access is via a pass that
  Companion already saved on this computer. If Unreal asks for one, it
  means the pass file got moved or deleted; ask your project lead for a
  fresh invite.
- **Closing the X just hides Companion.** It keeps running in the system
  tray (look for the bullet icon next to the clock). To fully quit,
  right-click the tray icon → **Quit**.
- **Adding another project** later: right-click the tray icon → **Add
  Project…**, then paste the next invite.

## Common problems

**"Windows protected your PC" — Run anyway not visible.**
Click **More info** on the blue screen first; the **Run anyway** button
appears underneath.

**"Permission denied"** during setup.
You clicked No on the Windows permission prompt. Click **Try again** in
Companion; on the next prompt, click **Yes**.

**"Can't reach the project server."**
Your internet may be down, or the project server is unreachable. Check
your internet, then click **Try again**. If it keeps happening, ping
your project lead.

**"Your access expired."**
Your invite is past its expiration date or your project lead revoked
your access. Ask for a fresh invite.

**Unreal says "Connection failed" after pasting the settings.**
Make sure all three lines (Server, User name, Workspace) match exactly
what Companion shows — Unreal trims spaces but the values are
case-sensitive. Click **Copy** in Companion rather than typing manually.

## Privacy + security

- The invite code contains credentials. Don't share it.
- Companion saves your access pass in your user profile, in plain text.
  This is the same place any Perforce-aware tool would put it (it's the
  standard Windows location). Anyone with access to your user account on
  this computer can read it; treat the machine the same way you treat
  your password manager.
- Companion does not phone home. No telemetry, no analytics, no update
  check. New versions ship as a new .msi from your project lead.
