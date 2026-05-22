# Contractor onboarding email — template

Copy-paste this and replace the four placeholders. Send via Signal or
encrypted email — the invite code contains credentials.

---

**Subject:** Welcome to the {{PROJECT_NAME}} project — quick setup

Hi {{CONTRACTOR_FIRST_NAME}},

Welcome aboard. Two things to get you set up — should take about 5 minutes.

**1. Install Spaceshop Companion**

Download the installer here: {{COMPANION_MSI_DOWNLOAD_LINK}}

When you run the installer, Windows may show a blue "Windows protected
your PC" screen. This is expected — we haven't paid for code-signing
yet. Click **More info** → **Run anyway** and you'll be on your way.

**2. Paste this invite code into Companion**

```
{{INVITE_CODE}}
```

Or just click this link (Companion will open with the code pre-loaded):

{{INVITE_URL}}

**What happens next**

- Companion opens and asks you to pick a folder for the project. The
  default is fine unless you have a preference.
- Click **Connect**.
- Windows will ask permission once — click **Yes**. Companion uses this
  to set up the connection service.
- After 2–5 minutes Companion shows you three lines (Server, User name,
  Workspace) and a big "Open project folder" button.
- Open Unreal Engine on the project. Go to **Editor Preferences → Source
  Control → Provider: Perforce**. Paste the three lines from Companion
  into the matching fields and click **Accept Settings**. Unreal will
  turn green ✓.

That's it. From then on, normal Unreal source control — check out, edit,
check in.

If anything looks off, ping me on {{CONTACT_CHANNEL}} and I'll sort it.

— Arsen
Spaceshop Studios

---

## Placeholders

| Token | Example |
|---|---|
| `{{PROJECT_NAME}}` | `Pharma Spot — Neymarc Q2` |
| `{{CONTRACTOR_FIRST_NAME}}` | `Sarah` |
| `{{COMPANION_MSI_DOWNLOAD_LINK}}` | A WeTransfer / Drive link to the latest .msi |
| `{{INVITE_CODE}}` | The base64 string from Workshop's invite generator |
| `{{INVITE_URL}}` | `spaceshop-companion://invite/<same-code>` |
| `{{CONTACT_CHANNEL}}` | `Signal +1 555 555 0123` or `arsen@spaceshop.studio` |
