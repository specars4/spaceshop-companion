# Contractor onboarding message — template

Copy-paste this and replace the three placeholders. Send via Signal or
encrypted email — the invite code contains credentials.

---

**Subject:** Welcome to the {{PROJECT_NAME}} project — quick setup

Hi {{CONTRACTOR_FIRST_NAME}},

Welcome aboard. Two things to get you set up — should take about 5 minutes.

**1. Install Spaceshop Companion**

Open this page and click **Download Companion**:

https://specars4.github.io/onboard/

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
- After 2–5 minutes you'll land on the project page. Click the big
  gold **Open in Unreal** button.
- Unreal Engine launches with Perforce already configured — Source
  Control should turn green automatically. No fiddling in Editor
  Preferences needed.

That's it. From then on, normal Unreal source control — check out, edit,
check in. Pull updates and submit work via Companion when you're not in
Unreal.

If anything looks off, ping me on {{CONTACT_CHANNEL}} and I'll sort it.

— Arsen
Spaceshop Studios

---

## Placeholders

| Token | Example |
|---|---|
| `{{PROJECT_NAME}}` | `Pharma Spot — Neymarc Q2` |
| `{{CONTRACTOR_FIRST_NAME}}` | `Sarah` |
| `{{INVITE_CODE}}` | The base64 string from Workshop's invite generator |
| `{{INVITE_URL}}` | `spaceshop-companion://invite/<same-code>` |
| `{{CONTACT_CHANNEL}}` | `Signal +1 555 555 0123` or `arsen@spaceshop.studio` |

## Operator notes

- The download link (`https://specars4.github.io/onboard/`)
  always points at the latest release — never update it per release.
- Contractors who installed an older Companion will auto-update on
  next launch; they don't need a new download link.
