# Spaceshop Companion — Contractor User Guide

A short guide. If anything here is wrong or confusing, tell your project
lead so they can fix it.

## Installing

1. Open the download link your project lead sent you and click
   **Download Companion**.
2. Run the downloaded `Spaceshop Companion ...msi` file.
3. Windows may show a blue **"Windows protected your PC"** screen. This
   is expected — Companion isn't code-signed yet. Click **More info** →
   **Run anyway**.
4. Companion installs to your user folder (no admin needed for the
   install itself). It opens automatically when done.

## First-time setup

1. Companion opens with a text box asking for your invite code.
2. Paste the invite (or click the `spaceshop-companion://...` link your
   project lead sent, which pre-fills it).
3. Click **Continue**.
4. Review the project name. Pick a folder where the project files should
   live — the default is fine in most cases. Click **Choose folder…**
   to override.
5. Click **Connect**.
6. **Windows will ask permission once.** Click **Yes**. This lets
   Companion set up the connection service (Tailscale).
7. Wait 2–5 minutes. You'll see progress steps fill in: parsing,
   preparing connection, joining the network, reaching the server,
   saving your access pass, setting up your workspace, downloading
   your project.

When done, you'll see the project page with a big gold **Open in
Unreal** button.

## Opening the project in Unreal

Click the gold **Open in Unreal** button on Companion's project page.

That's it. Companion finds the `.uproject` in your project folder,
writes the Perforce settings into Unreal's config for you, then launches
Unreal Engine. When Unreal opens, the Source Control indicator should
already be green — no manual setup in Editor Preferences needed.

If the button shows an error like "No .uproject found", run **Pull
latest** first to download the project content, then try again.

Use Unreal's normal source-control menu to check out, edit, and check in
assets. Companion can stay running in the tray (bottom-right corner of
the screen) — it's there if you need to pull updates or submit between
Unreal sessions.

## Daily use

- **Pull latest**: click **Re-check & pull** on the project page to get
  new work from the server. Shows a green banner if you're already up to
  date. While pulling you'll see a live file count + the current
  filename below the badges, so you know it's actually working.
- **Submit your work**: click **Review & submit changes** to see a list
  of files you've changed, type a short description, and send them to
  the server.
- **Open project folder**: shortcut to the local folder in Explorer.

## The tray icon

The dark plug icon in the system tray (bottom-right corner) changes
color based on what's working:

- **Gold/green** — Tailscale is up AND your project server is reachable
- **Yellow** — partially working (one of those isn't responding)
- **Red** — both broken/offline

Right-click the tray icon for:
- **Open Companion** — bring the main window forward
- **Add Project…** — paste another invite
- **Sync \<Project\>** — pull latest for that project right from the
  tray (you don't have to open the main window)
- **Quit** — fully close Companion

Hovering over the icon shows a tooltip with how many projects you have
and how many changes are pending across them.

## If something looks stuck — Repair workspace

In **Advanced** on the project page there's a **Repair workspace**
button. Use this when files look out of sync but Pull Latest isn't
fixing it (e.g. you accidentally deleted some files in Explorer, or a
sync got interrupted mid-way).

What it does: removes any file in the project folder that isn't tracked
by the server, then re-pulls anything missing.

What's safe: files you've opened for add or edit in p4 (anything that
shows up in **Review & submit**) stay put. Pending changes are not
touched.

What gets removed: anything you dropped into the project folder but
never added to p4 — notes, screenshots, scratch exports, render
outputs. If you have stuff like that you want to keep, move it
somewhere outside the project folder first, or cancel and add it to
p4 first.

You'll see a confirmation dialog explaining this before anything happens.

## Uninstalling

When you're done with a Spaceshop project for good, you have two ways
to remove Companion:

- **From the Project page** → Advanced section → **Uninstall Spaceshop
  Companion** — this is the clean option. Removes all Spaceshop data
  (projects, your access pass to the server, cached binaries), and
  optionally Tailscale too:
  - If Companion installed Tailscale for you originally, the checkbox
    defaults to ON — Tailscale gets uninstalled along with everything else
  - If you had Tailscale before Companion (for personal use or other
    apps), the checkbox defaults to OFF — Tailscale stays, your other
    uses keep working
  - You can change the checkbox either way before confirming
  
  Type **YES** to confirm, and Companion handles the rest.
- **From Windows Settings → Apps & Features** — quicker but messier.
  Removes only Companion itself. Tailscale, your p4 ticket, and
  Companion's data folder will be left behind. Use the in-app option
  above if you want a complete cleanup.

## Updates

Companion checks for new versions on launch and shows a banner at the
top if one's available. Click **Install** — Companion downloads the
update, installs it, and restarts itself. No re-onboarding needed; your
projects come back exactly as they were.

## Things to know

- **You don't have a password.** Your access is via a pass that
  Companion already saved on this computer. If Unreal asks for one, it
  means the pass file got moved or deleted; ask your project lead for a
  fresh invite.
- **Closing the X just hides Companion.** It keeps running in the system
  tray (look for the dark plug icon next to the clock). To fully quit,
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

**"No .uproject found in <folder>."**
Click **Re-check & pull** first to download the project content from
the server, then try **Open in Unreal** again.

**Unreal opens but Source Control isn't green.**
Click the Source Control icon in Unreal's bottom-right corner → Change
Source Control Settings. The Provider should already be Perforce and
the Server/User/Workspace already filled in. Click **Accept Settings**
to retry the connection.

## Privacy + security

- The invite code contains credentials. Don't share it.
- Companion saves your access pass in your user profile, in plain text.
  This is the same place any Perforce-aware tool would put it (it's the
  standard Windows location). Anyone with access to your user account on
  this computer can read it; treat the machine the same way you treat
  your password manager.
- Companion only contacts the project server (for source control) and
  GitHub Releases (to check for updates). No telemetry, no analytics.
